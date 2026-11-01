//! The projection: state rebuilt from events (§7.4, §7.5, §7.6).
//!
//! [`rebuild`] is a pure function of (events, keyring): the same input gives
//! byte-identical output on every device. Phases:
//! 1. organizational events, each checked against the state of its own
//!    causal past (the [`AuthState`] of its organizational ancestors);
//! 2. pre-pass on conflicts between concurrent operations on the same object;
//! 3. every other event, in `(depth, id)` order, checked against the
//!    authorization state of its causal past and the current object state.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use core_crypto::{ChannelId, ChannelKeyring};
use core_events::genesis::check_genesis;
use core_events::{EventError, EventHash, EventKind, OrgId, SignedEvent};
use core_model::config::Trigger;
use core_model::{Id, PersonId, Slug, TextKey};
use serde::Serialize;
use time::Date;

use crate::auth::AuthState;
use crate::channels::ChannelInfo;
use crate::config::{Cfg, Configs};
use crate::ops::{
    AggregateSnapshot, Assignee, BatchKey, DocumentContent, NoticeItem, Op, ProgramSnapshot,
    Response, SlotKey, Values,
};
use crate::time::{PeriodKey, date_of};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Verdict {
    Valid,
    Invalid(String),
    /// Encrypted for a channel this device has no key for.
    PendingKey,
    /// Lost a conflict against a concurrent event.
    Superseded,
    /// From a device revoked concurrently, declared after the revocation.
    Quarantined,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepRecord {
    pub step: Slug,
    pub from: Slug,
    pub to: Slug,
    pub by: Option<PersonId>,
    pub date: Date,
    pub note: Option<String>,
    pub event: EventHash,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Document {
    pub content: DocumentContent,
    pub state: Slug,
    /// Increases with every content change after creation.
    pub version: u32,
    pub created_by: Option<PersonId>,
    pub history: Vec<StepRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BatchRecord {
    pub step: Slug,
    pub documents: Vec<Id>,
    pub by: PersonId,
    pub date: Date,
    pub event: EventHash,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Batch {
    pub records: Vec<BatchRecord>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Aggregate {
    pub generated: Option<AggregateSnapshot>,
    pub distributed: Option<AggregateSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProgramStatus {
    Draft,
    Assigned,
}

/// A program as its team sees it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Program {
    pub program_type: Slug,
    pub config_version: u32,
    pub occurrences: Vec<Date>,
    pub assignments: BTreeMap<SlotKey, (Assignee, Values)>,
    pub status: ProgramStatus,
    pub sent_version: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Holding {
    pub holder: PersonId,
    pub since: Date,
    pub until: Option<Date>,
    pub logs: Vec<(Date, Values)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resource {
    pub resource_type: Slug,
    pub attributes: Values,
    pub holding: Option<Holding>,
    pub history: Vec<Holding>,
    pub retired: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    pub resource: Id,
    pub resource_type: Slug,
    pub holder: PersonId,
    pub request_type: Slug,
    pub due: Date,
    pub answer: Option<Slug>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum BoardItem {
    Notice { title: String, body: String },
    Program(ProgramSnapshot),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoardPost {
    pub audience: Slug,
    pub item: BoardItem,
    pub from: Date,
    pub until: Date,
    pub withdrawn: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AlertKind {
    InvalidEvent,
    Quarantined,
    Superseded,
    ConfigConflict,
    DoubleAssignment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Alert {
    pub kind: AlertKind,
    pub event: Option<EventHash>,
    pub message: String,
}

/// A notification produced by an event, with its resolved recipients.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TriggerRecord {
    pub trigger: Option<Trigger>,
    pub text: TextKey,
    pub event: EventHash,
    pub date: Date,
    pub recipients: BTreeSet<PersonId>,
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Projection {
    pub org: OrgId,
    /// Authorization state from every valid organizational event.
    pub auth: AuthState,
    #[serde(skip)]
    pub configs: Configs,
    /// Active configuration version (§6.6).
    pub active_config: u32,
    pub verdicts: BTreeMap<EventHash, Verdict>,
    pub channels: BTreeMap<ChannelId, ChannelInfo>,
    pub documents: BTreeMap<Id, Document>,
    pub batches: BTreeMap<BatchKey, Batch>,
    pub aggregates: BTreeMap<(Slug, PeriodKey), Aggregate>,
    pub calendar: BTreeMap<Id, crate::ops::CalendarEventUpserted>,
    pub programs: BTreeMap<Id, Program>,
    pub assigned_snapshots: BTreeMap<Id, ProgramSnapshot>,
    pub published: BTreeMap<Id, ProgramSnapshot>,
    pub notices: BTreeMap<(Id, PersonId), (Slug, Vec<NoticeItem>)>,
    pub responses: BTreeMap<(Id, SlotKey), Response>,
    pub resources: BTreeMap<Id, Resource>,
    pub requests: BTreeMap<Id, Request>,
    pub board: BTreeMap<Id, BoardPost>,
    pub alerts: Vec<Alert>,
    pub triggers: Vec<TriggerRecord>,
    /// Events of the projection with their decoded operation (valid only).
    #[serde(skip)]
    pub heads: Vec<(EventHash, u64)>,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("no events")]
    Empty,
    #[error("the first event is not a valid genesis: {0}")]
    Genesis(EventError),
    #[error("the genesis configuration cannot be decoded: {0}")]
    Config(String),
}

/// Graph helpers over the event slice.
pub(crate) struct Graph<'a> {
    pub events: &'a [SignedEvent],
    pub parents: Vec<Vec<usize>>,
}

impl<'a> Graph<'a> {
    fn new(events: &'a [SignedEvent]) -> Self {
        let index: HashMap<EventHash, usize> =
            events.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
        let parents = events
            .iter()
            .map(|e| {
                e.body
                    .parents
                    .iter()
                    .filter_map(|p| index.get(p).copied())
                    .collect()
            })
            .collect();
        Self { events, parents }
    }

    /// True when event `a` is in the causal past of event `b`.
    pub fn is_ancestor(&self, a: usize, b: usize) -> bool {
        let target_depth = self.events[a].body.depth;
        let mut stack = vec![b];
        let mut seen = BTreeSet::new();
        while let Some(x) = stack.pop() {
            for &p in &self.parents[x] {
                if p == a {
                    return true;
                }
                if self.events[p].body.depth > target_depth && seen.insert(p) {
                    stack.push(p);
                }
            }
        }
        false
    }

    pub fn concurrent(&self, a: usize, b: usize) -> bool {
        a != b && !self.is_ancestor(a, b) && !self.is_ancestor(b, a)
    }
}

/// Bitset of organizational ancestors.
type Bits = Vec<u64>;

fn bit_set(bits: &mut Bits, i: usize) {
    let w = i / 64;
    if bits.len() <= w {
        bits.resize(w + 1, 0);
    }
    bits[w] |= 1 << (i % 64);
}

fn bit_get(bits: &Bits, i: usize) -> bool {
    bits.get(i / 64).is_some_and(|w| w & (1 << (i % 64)) != 0)
}

fn bit_or(a: &mut Bits, b: &Bits) {
    if a.len() < b.len() {
        a.resize(b.len(), 0);
    }
    for (x, y) in a.iter_mut().zip(b) {
        *x |= y;
    }
}

pub(crate) struct Decoded {
    pub op: Option<Op>,
    /// Content present but undecodable.
    pub malformed: bool,
}

/// Inputs shared by the phases.
pub(crate) struct Ctx<'a> {
    pub graph: Graph<'a>,
    pub decoded: Vec<Decoded>,
    pub auth_bit: Vec<Option<usize>>,
    pub auth_events: Vec<usize>,
    pub past: Vec<Bits>,
    pub cache: HashMap<Bits, Arc<AuthState>>,
    pub verdicts: Vec<Option<Verdict>>,
}

impl Ctx<'_> {
    /// Authorization state of the causal past of event `i`.
    pub fn auth_past(&mut self, i: usize, configs: &Configs) -> Arc<AuthState> {
        let bits = self.past[i].clone();
        if let Some(s) = self.cache.get(&bits) {
            return s.clone();
        }
        let mut state = AuthState::default();
        for (bit, &ev) in self.auth_events.iter().enumerate() {
            if !bit_get(&bits, bit) || self.verdicts[ev] != Some(Verdict::Valid) {
                continue;
            }
            if let Some(op) = &self.decoded[ev].op {
                let cfg = configs
                    .get(state.config_version.max(1))
                    .or_else(|| configs.latest());
                if let Some(cfg) = cfg {
                    let cfg = cfg.clone();
                    state.apply(&cfg, self.graph.events[ev].id, op);
                }
            }
        }
        let state = Arc::new(state);
        self.cache.insert(bits, state.clone());
        state
    }
}

impl Projection {
    pub fn cfg(&self) -> Option<&Cfg> {
        self.configs
            .get(self.active_config)
            .or_else(|| self.configs.latest())
    }

    pub fn verdict(&self, id: &EventHash) -> Option<&Verdict> {
        self.verdicts.get(id)
    }

    /// Stable hash of the whole projection (convergence tests).
    pub fn state_hash(&self) -> [u8; 32] {
        let bytes = postcard::to_allocvec(self).unwrap_or_default();
        *blake3::hash(&bytes).as_bytes()
    }

    pub fn document_by(
        &self,
        form: &Slug,
        subject: PersonId,
        period: PeriodKey,
    ) -> Option<(&Id, &Document)> {
        self.documents.iter().find(|(_, d)| {
            &d.content.form == form && d.content.subject == subject && d.content.period == period
        })
    }

    fn alert(&mut self, kind: AlertKind, event: Option<EventHash>, message: impl Into<String>) {
        self.alerts.push(Alert {
            kind,
            event,
            message: message.into(),
        });
    }
}

/// Rebuilds the projection from scratch.
pub fn rebuild(
    events: &[SignedEvent],
    keyring: &ChannelKeyring,
) -> Result<Projection, EngineError> {
    let mut events: Vec<SignedEvent> = events.to_vec();
    core_events::order::sort_events(&mut events);
    events.dedup_by(|a, b| a.id == b.id);
    let genesis = events.first().ok_or(EngineError::Empty)?;
    let info = check_genesis(genesis).map_err(EngineError::Genesis)?;
    let compiled = core_config::decode(&info.payload.config_bundle)
        .map_err(|e| EngineError::Config(e.to_string()))?;
    let mut configs = Configs::default();
    configs.versions.insert(1, Cfg::new(1, compiled));
    let org = genesis.org_id();

    let graph = Graph::new(&events);
    let n = events.len();
    let mut decoded = Vec::with_capacity(n);
    for e in &events {
        let d = match e.open(keyring) {
            Ok(bytes) => match Op::decode(e.body.kind, &bytes) {
                Some(op) => Decoded {
                    op: Some(op),
                    malformed: false,
                },
                None => Decoded {
                    op: None,
                    malformed: true,
                },
            },
            Err(_) => Decoded {
                op: None,
                malformed: false,
            },
        };
        decoded.push(d);
    }
    let mut auth_bit = vec![None; n];
    let mut auth_events = Vec::new();
    for (i, e) in events.iter().enumerate() {
        if Op::is_authorization(e.body.kind) {
            auth_bit[i] = Some(auth_events.len());
            auth_events.push(i);
        }
    }
    let mut past: Vec<Bits> = Vec::with_capacity(n);
    for i in 0..n {
        let mut bits = Bits::new();
        for &p in &graph.parents[i] {
            bit_or(&mut bits, &past[p]);
            if let Some(b) = auth_bit[p] {
                bit_set(&mut bits, b);
            }
        }
        past.push(bits);
    }
    let mut ctx = Ctx {
        graph,
        decoded,
        auth_bit,
        auth_events,
        past,
        cache: HashMap::new(),
        verdicts: vec![None; n],
    };

    let mut proj = Projection {
        org,
        auth: AuthState::default(),
        configs,
        active_config: 1,
        verdicts: BTreeMap::new(),
        channels: BTreeMap::new(),
        documents: BTreeMap::new(),
        batches: BTreeMap::new(),
        aggregates: BTreeMap::new(),
        calendar: BTreeMap::new(),
        programs: BTreeMap::new(),
        assigned_snapshots: BTreeMap::new(),
        published: BTreeMap::new(),
        notices: BTreeMap::new(),
        responses: BTreeMap::new(),
        resources: BTreeMap::new(),
        requests: BTreeMap::new(),
        board: BTreeMap::new(),
        alerts: Vec::new(),
        triggers: Vec::new(),
        heads: Vec::new(),
    };

    // Phase 1: organizational events.
    let auth_list = ctx.auth_events.clone();
    for &i in &auth_list {
        let verdict = organizational_verdict(&mut ctx, &mut proj, i);
        ctx.verdicts[i] = Some(verdict);
    }
    let all: Bits = {
        let mut b = Bits::new();
        for (bit, &ev) in ctx.auth_events.iter().enumerate() {
            if ctx.verdicts[ev] == Some(Verdict::Valid) {
                bit_set(&mut b, bit);
            }
        }
        b
    };
    let global = {
        let mut s = AuthState::default();
        for (bit, &ev) in ctx.auth_events.iter().enumerate() {
            if bit_get(&all, bit) {
                if let (Some(op), Some(cfg)) = (
                    &ctx.decoded[ev].op,
                    proj.configs.get(s.config_version.max(1)).cloned(),
                ) {
                    s.apply(&cfg, ctx.graph.events[ev].id, op);
                }
            }
        }
        s
    };
    for &i in &auth_list {
        let id = ctx.graph.events[i].id;
        if global.superseded_roles.contains(&id) {
            ctx.verdicts[i] = Some(Verdict::Superseded);
            proj.alert(
                AlertKind::Superseded,
                Some(id),
                "role grant beyond the maximum number of holders",
            );
        }
    }
    proj.active_config = active_config(&ctx, &mut proj);
    proj.auth = global;

    // Revocations, for quarantine.
    let revocations: Vec<(usize, core_crypto::DevicePublicKey)> = auth_list
        .iter()
        .filter(|&&i| ctx.verdicts[i] == Some(Verdict::Valid))
        .filter_map(|&i| match &ctx.decoded[i].op {
            Some(Op::DeviceRevoked(r)) => Some((i, r.device)),
            _ => None,
        })
        .collect();
    let accepted = accepted_quarantines(&ctx, &proj);

    // Phase 2: conflicts.
    let superseded = crate::conflicts::superseded(&mut ctx, &proj);

    // Phase 3: everything else.
    for i in 0..n {
        if ctx.verdicts[i].is_some() {
            continue;
        }
        let e = &ctx.graph.events[i];
        let id = e.id;
        // Quarantine depends only on author, time and causal past: decided
        // before decryption, so every device agrees.
        let quarantined = !accepted.contains(&id)
            && revocations.iter().any(|(r, dev)| {
                *dev == e.body.author
                    && !ctx.auth_bit[*r].is_some_and(|b| bit_get(&ctx.past[i], b))
                    && e.body.declared_time > ctx.graph.events[*r].body.declared_time
            });
        let verdict = if quarantined {
            Verdict::Quarantined
        } else if ctx.decoded[i].malformed {
            Verdict::Invalid("content cannot be decoded".into())
        } else if ctx.decoded[i].op.is_none() {
            Verdict::PendingKey
        } else if superseded.contains(&i) {
            Verdict::Superseded
        } else {
            let auth = ctx.auth_past(i, &proj.configs);
            let op = ctx.decoded[i].op.clone();
            match op {
                Some(op) => match crate::handle::handle(&mut proj, &auth, &ctx, i, &op) {
                    Ok(()) => Verdict::Valid,
                    Err(d) => Verdict::Invalid(d.0),
                },
                None => Verdict::PendingKey,
            }
        };
        match &verdict {
            Verdict::Invalid(m) => proj.alert(AlertKind::InvalidEvent, Some(id), m.clone()),
            Verdict::Quarantined => proj.alert(
                AlertKind::Quarantined,
                Some(id),
                "event from a device revoked concurrently",
            ),
            Verdict::Superseded => proj.alert(
                AlertKind::Superseded,
                Some(id),
                "lost a conflict against a concurrent event",
            ),
            _ => {}
        }
        ctx.verdicts[i] = Some(verdict);
    }

    for (i, v) in ctx.verdicts.iter().enumerate() {
        proj.verdicts.insert(
            ctx.graph.events[i].id,
            v.clone().unwrap_or(Verdict::PendingKey),
        );
    }
    // Heads of the whole graph, for new local events.
    let mut has_child = vec![false; n];
    for ps in &ctx.graph.parents {
        for &p in ps {
            has_child[p] = true;
        }
    }
    proj.heads = (0..n)
        .filter(|i| !has_child[*i])
        .map(|i| (ctx.graph.events[i].id, ctx.graph.events[i].body.depth))
        .collect();
    Ok(proj)
}

fn organizational_verdict(ctx: &mut Ctx<'_>, proj: &mut Projection, i: usize) -> Verdict {
    let e = &ctx.graph.events[i];
    if e.is_genesis() {
        return if i == 0 {
            Verdict::Valid
        } else {
            Verdict::Invalid("second genesis".into())
        };
    }
    if ctx.decoded[i].malformed {
        return Verdict::Invalid("content cannot be decoded".into());
    }
    let Some(op) = ctx.decoded[i].op.clone() else {
        return Verdict::PendingKey;
    };
    let date = date_of(e.body.declared_time);
    let kind = e.body.kind;
    let author_key = e.body.author;
    let channel = e.body.channel;
    let org = proj.org;
    let auth = ctx.auth_past(i, &proj.configs);
    let Some(cfg) = proj.configs.get(auth.config_version.max(1)).cloned() else {
        return Verdict::Invalid("unknown configuration version".into());
    };
    let author = match auth.author(&author_key) {
        Ok(a) => a,
        Err(d) => return Verdict::Invalid(d.0),
    };
    if !kind.is_public() {
        let expected = ChannelId::derive(&org.0, &core_crypto::ChannelDescriptor::Organization);
        let ok = matches!((channel, expected), (core_events::PayloadChannel::Channel(c), Ok(x)) if c.id == x);
        if !ok {
            return Verdict::Invalid("organizational events go in the organization channel".into());
        }
    }
    match auth.check(&cfg, author, date, &op) {
        Ok(()) => {
            if let Op::ConfigPublished(c) = &op {
                if let Ok(compiled) = core_config::decode(&c.bundle) {
                    proj.configs
                        .versions
                        .insert(c.version, Cfg::new(c.version, compiled));
                }
            }
            Verdict::Valid
        }
        Err(d) => {
            let _ = kind == EventKind::OrgGenesis;
            Verdict::Invalid(d.0)
        }
    }
}

/// Active configuration: the latest `config.published` that has every other
/// one in its causal past. With concurrent heads, the latest common one
/// stays active and an alert is raised (§7.6).
fn active_config(ctx: &Ctx<'_>, proj: &mut Projection) -> u32 {
    let configs: Vec<(usize, u32)> = ctx
        .auth_events
        .iter()
        .filter(|&&i| ctx.verdicts[i] == Some(Verdict::Valid))
        .filter_map(|&i| match &ctx.decoded[i].op {
            Some(Op::ConfigPublished(c)) => Some((i, c.version)),
            _ => None,
        })
        .collect();
    let heads: Vec<(usize, u32)> = configs
        .iter()
        .filter(|(i, _)| {
            !configs
                .iter()
                .any(|(j, _)| j != i && ctx.graph.is_ancestor(*i, *j))
        })
        .copied()
        .collect();
    match heads.as_slice() {
        [] => 1,
        [(_, v)] => *v,
        _ => {
            proj.alert(
                AlertKind::ConfigConflict,
                None,
                "concurrent configuration versions: publish one that includes both",
            );
            configs
                .iter()
                .filter(|(i, _)| heads.iter().all(|(h, _)| ctx.graph.is_ancestor(*i, *h)))
                .map(|(_, v)| *v)
                .max()
                .unwrap_or(1)
        }
    }
}

fn accepted_quarantines(ctx: &Ctx<'_>, proj: &Projection) -> BTreeSet<EventHash> {
    let mut out = BTreeSet::new();
    for (i, e) in ctx.graph.events.iter().enumerate() {
        if let Some(Op::QuarantineResolved(q)) = &ctx.decoded[i].op {
            let root = proj
                .auth
                .author(&e.body.author)
                .is_ok_and(|p| proj.auth.is_root(p));
            if root && q.accept {
                out.insert(q.event);
            }
        }
    }
    out
}
