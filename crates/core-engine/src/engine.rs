//! Projection kept up to date as events arrive (§7.5).
//!
//! [`Engine`] gives exactly the result of [`crate::rebuild`] on the same
//! events and keyring, but a new event usually costs only its own
//! processing:
//! - events that sort after everything already applied are applied on the
//!   current state ([`Update::Appended`]);
//! - an event that sorts earlier (e.g. from a device that was offline)
//!   resumes from the nearest saved state before it ([`Update::Replayed`]);
//! - organizational events, quarantine resolutions and keys that open
//!   events this device could not read rebuild everything
//!   ([`Update::Rebuilt`]): they can change the verdict of any event.

use std::collections::BTreeSet;

use core_crypto::{ChannelKeyring, DeviceKeys, DevicePublicKey};
use core_events::order::{order_key, sort_events};
use core_events::{EventHash, PayloadChannel, SignedEvent};

use crate::channels::absorb_keys;
use crate::ops::Op;
use crate::projection::{
    Bits, Ctx, Decoded, EngineError, Graph, Projection, Verdict, accepted_quarantines, apply_event,
    decode, genesis_setup, heads, organizational_phase, revocations,
};

/// Events applied between two saved states.
const CHECKPOINT_EVERY: usize = 256;
/// Saved states kept (the most recent ones); older events resume from the
/// state after the organizational phase.
const CHECKPOINTS_KEPT: usize = 8;

type OrderKey = (u64, EventHash);

/// What [`Engine::extend`] had to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    Unchanged,
    Appended,
    Replayed,
    Rebuilt,
}

pub struct Engine {
    ctx: Ctx,
    ring: ChannelKeyring,
    /// State after the organizational phase, before any other event.
    base: Projection,
    proj: Projection,
    revocations: Vec<(EventHash, DevicePublicKey)>,
    accepted: BTreeSet<EventHash>,
    superseded: BTreeSet<EventHash>,
    /// State before the event with this key.
    checkpoints: Vec<(OrderKey, Projection)>,
    since_checkpoint: usize,
    checkpoint_every: usize,
}

impl Engine {
    /// Builds the projection from scratch.
    pub fn build(mut events: Vec<SignedEvent>, ring: ChannelKeyring) -> Result<Self, EngineError> {
        sort_events(&mut events);
        events.dedup_by(|a, b| a.id == b.id);
        let (configs, org) = genesis_setup(&events)?;
        let decoded = events.iter().map(|e| decode(e, &ring)).collect();
        let mut ctx = Ctx::new(Graph::new(events), decoded);
        let base = organizational_phase(&mut ctx, configs, org);
        let revocations = revocations(&ctx);
        let accepted = accepted_quarantines(&ctx, &base);
        let superseded = superseded_ids(&mut ctx, &base);
        let mut engine = Self {
            ctx,
            ring,
            proj: base.clone(),
            base,
            revocations,
            accepted,
            superseded,
            checkpoints: Vec::new(),
            since_checkpoint: 0,
            checkpoint_every: CHECKPOINT_EVERY,
        };
        engine.replay_from(0);
        Ok(engine)
    }

    /// Builds from the events, with the keys this device can open in them.
    pub fn build_for(events: Vec<SignedEvent>, keys: &DeviceKeys) -> Result<Self, EngineError> {
        let ring = crate::channels::derive_keyring(&events, keys);
        Self::build(events, ring)
    }

    /// Events between saved states (default 256); small values exercise
    /// the resume path in tests.
    pub fn set_checkpoint_interval(&mut self, events: usize) {
        self.checkpoint_every = events.max(1);
    }

    pub fn projection(&self) -> &Projection {
        &self.proj
    }

    pub fn into_projection(self) -> Projection {
        self.proj
    }

    /// Every event, in the deterministic order.
    pub fn events(&self) -> &[SignedEvent] {
        &self.ctx.graph.events
    }

    pub fn keyring(&self) -> &ChannelKeyring {
        &self.ring
    }

    pub fn contains(&self, id: &EventHash) -> bool {
        self.ctx.graph.index.contains_key(id)
    }

    /// Adds events (any order, duplicates ignored), taking the keys this
    /// device can open from them.
    pub fn extend(
        &mut self,
        new: &[SignedEvent],
        keys: &DeviceKeys,
    ) -> Result<Update, EngineError> {
        let mut ring = self.ring.clone();
        absorb_keys(&mut ring, new, keys);
        self.extend_with_ring(new, ring)
    }

    /// Adds events with an explicit keyring (a superset of the current one).
    pub fn extend_with_ring(
        &mut self,
        new: &[SignedEvent],
        ring: ChannelKeyring,
    ) -> Result<Update, EngineError> {
        let mut fresh: Vec<SignedEvent> = new
            .iter()
            .filter(|e| !self.contains(&e.id))
            .cloned()
            .collect();
        sort_events(&mut fresh);
        fresh.dedup_by(|a, b| a.id == b.id);
        let added = ring.added_since(&self.ring);
        if fresh.is_empty() && added.is_empty() {
            return Ok(Update::Unchanged);
        }
        let decoded: Vec<Decoded> = fresh.iter().map(|e| decode(e, &ring)).collect();
        if self.unlocks_pending(&added) || needs_rebuild(&fresh, &decoded) {
            let mut all = self.ctx.graph.events.clone();
            all.extend(fresh);
            let every = self.checkpoint_every;
            *self = Self::build(all, ring)?;
            self.checkpoint_every = every;
            return Ok(Update::Rebuilt);
        }
        self.ring = ring;
        let Some(first_new) = fresh.first().map(order_key) else {
            return Ok(Update::Unchanged);
        };
        let last_old = self.ctx.graph.events.last().map(order_key);

        // Causal past of the organizational events, to detect a missing
        // ancestor arriving late (their verdicts depend on it).
        let auth_past: Vec<(EventHash, Bits)> = self
            .ctx
            .auth_events
            .iter()
            .map(|&i| (self.ctx.graph.events[i].id, self.ctx.past[i].clone()))
            .collect();
        let old_events = std::mem::take(&mut self.ctx.graph.events);
        let old_decoded = std::mem::take(&mut self.ctx.decoded);
        let (events, decoded) = merge(old_events, old_decoded, fresh, decoded);
        self.ctx.graph = Graph::new(events);
        self.ctx.decoded = decoded;
        self.reindex();
        let auth_moved = auth_past.iter().any(|(id, bits)| {
            self.ctx
                .graph
                .index
                .get(id)
                .is_none_or(|&i| !same_bits(&self.ctx.past[i], bits))
        });
        if auth_moved {
            let every = self.checkpoint_every;
            let all = std::mem::take(&mut self.ctx.graph.events);
            let ring = self.ring.clone();
            *self = Self::build(all, ring)?;
            self.checkpoint_every = every;
            return Ok(Update::Rebuilt);
        }

        // A new event can make an older one lose a conflict (or win it).
        let superseded = superseded_ids(&mut self.ctx, &self.base);
        let changed = superseded
            .symmetric_difference(&self.superseded)
            .filter_map(|id| self.key_of(id))
            .min();
        self.superseded = superseded;
        let start = changed.map_or(first_new, |c| c.min(first_new));

        if last_old.is_some_and(|last| start > last) {
            let from = self.position(start);
            self.replay_from(from);
            Ok(Update::Appended)
        } else {
            let from = match self.restore(start) {
                Some(key) => self.position(key),
                None => 0,
            };
            self.replay_from(from);
            Ok(Update::Replayed)
        }
    }

    /// Verdicts that new local events would get, without keeping them
    /// (dry run of an action).
    pub fn preview(
        &mut self,
        new: &[SignedEvent],
        ring: &ChannelKeyring,
    ) -> Result<Vec<Verdict>, EngineError> {
        let added = ring.added_since(&self.ring);
        let decoded: Vec<Decoded> = new.iter().map(|e| decode(e, ring)).collect();
        let last_old = self.ctx.graph.events.last().map(order_key);
        let in_order = new
            .iter()
            .zip(new.iter().skip(1))
            .all(|(a, b)| order_key(a) < order_key(b))
            && new
                .first()
                .is_some_and(|e| last_old.is_none_or(|l| order_key(e) > l));
        let fast = in_order
            && !new.iter().any(|e| self.contains(&e.id))
            && !self.unlocks_pending(&added)
            && !needs_rebuild(new, &decoded);
        if fast {
            if let Some(verdicts) = self.preview_appended(new, decoded) {
                return Ok(verdicts);
            }
        }
        let mut all = self.ctx.graph.events.clone();
        all.extend(new.iter().cloned());
        let proj = Self::build(all, ring.clone())?.into_projection();
        Ok(new
            .iter()
            .map(|e| proj.verdict(&e.id).cloned().unwrap_or(Verdict::PendingKey))
            .collect())
    }

    /// Appends temporarily, applies on a copy of the state, then removes
    /// the events again. `None` when the events change older verdicts.
    fn preview_appended(
        &mut self,
        new: &[SignedEvent],
        decoded: Vec<Decoded>,
    ) -> Option<Vec<Verdict>> {
        let n = self.ctx.graph.events.len();
        for (e, d) in new.iter().zip(decoded) {
            let i = self.ctx.graph.events.len();
            let parents = e
                .body
                .parents
                .iter()
                .filter_map(|p| self.ctx.graph.index.get(p).copied())
                .collect::<Vec<_>>();
            let mut bits = Bits::new();
            for &p in &parents {
                crate::projection::bit_or(&mut bits, &self.ctx.past[p]);
                if let Some(b) = self.ctx.auth_bit[p] {
                    crate::projection::bit_set(&mut bits, b);
                }
            }
            self.ctx.graph.index.insert(e.id, i);
            self.ctx.graph.parents.push(parents);
            self.ctx.graph.events.push(e.clone());
            self.ctx.decoded.push(d);
            self.ctx.auth_bit.push(None);
            self.ctx.past.push(bits);
            self.ctx.verdicts.push(None);
        }
        let superseded = superseded_ids(&mut self.ctx, &self.base);
        let old_changed = superseded
            .symmetric_difference(&self.superseded)
            .any(|id| self.ctx.graph.index.get(id).is_some_and(|&i| i < n));
        let result = (!old_changed).then(|| {
            let mut proj = self.proj.clone();
            (n..n + new.len())
                .map(|i| {
                    apply_event(
                        &mut self.ctx,
                        &mut proj,
                        i,
                        &self.revocations,
                        &self.accepted,
                        &superseded,
                    )
                })
                .collect()
        });
        for e in new {
            self.ctx.graph.index.remove(&e.id);
        }
        self.ctx.graph.events.truncate(n);
        self.ctx.graph.parents.truncate(n);
        self.ctx.decoded.truncate(n);
        self.ctx.auth_bit.truncate(n);
        self.ctx.past.truncate(n);
        self.ctx.verdicts.truncate(n);
        result
    }

    /// True when a new key opens an event that was waiting for it.
    fn unlocks_pending(&self, added: &[core_crypto::ChannelRef]) -> bool {
        !added.is_empty()
            && self
                .ctx
                .graph
                .events
                .iter()
                .zip(&self.ctx.decoded)
                .any(|(e, d)| {
                    d.op.is_none()
                        && !d.malformed
                        && matches!(e.body.channel, PayloadChannel::Channel(c) if added.contains(&c))
                })
    }

    /// Recomputes the indices after a merge; organizational verdicts do not
    /// change (no new organizational event reaches this point).
    fn reindex(&mut self) {
        self.ctx.index();
        for k in 0..self.ctx.auth_events.len() {
            let i = self.ctx.auth_events[k];
            let id = self.ctx.graph.events[i].id;
            self.ctx.verdicts[i] = self.base.verdicts.get(&id).cloned();
        }
    }

    fn key_of(&self, id: &EventHash) -> Option<OrderKey> {
        self.ctx
            .graph
            .index
            .get(id)
            .map(|&i| order_key(&self.ctx.graph.events[i]))
    }

    fn position(&self, key: OrderKey) -> usize {
        self.ctx
            .graph
            .events
            .partition_point(|e| order_key(e) < key)
    }

    /// Goes back to the latest saved state at or before `key`; returns its
    /// key (`None` = the state after the organizational phase).
    fn restore(&mut self, key: OrderKey) -> Option<OrderKey> {
        while self.checkpoints.last().is_some_and(|(k, _)| *k > key) {
            self.checkpoints.pop();
        }
        self.since_checkpoint = 0;
        match self.checkpoints.last() {
            Some((k, p)) => {
                self.proj = p.clone();
                Some(*k)
            }
            None => {
                self.proj = self.base.clone();
                None
            }
        }
    }

    /// Applies every non-organizational event from position `start` on the
    /// current state (which must be the state before `start`).
    fn replay_from(&mut self, start: usize) {
        let n = self.ctx.graph.events.len();
        for i in start..n {
            if self.ctx.auth_bit[i].is_some() {
                continue;
            }
            if self.since_checkpoint >= self.checkpoint_every {
                let key = order_key(&self.ctx.graph.events[i]);
                self.checkpoints.push((key, self.proj.clone()));
                if self.checkpoints.len() > CHECKPOINTS_KEPT {
                    self.checkpoints.remove(0);
                }
                self.since_checkpoint = 0;
            }
            self.since_checkpoint += 1;
            apply_event(
                &mut self.ctx,
                &mut self.proj,
                i,
                &self.revocations,
                &self.accepted,
                &self.superseded,
            );
        }
        self.proj.heads = heads(&self.ctx);
    }
}

/// Organizational events and quarantine resolutions can change the verdict
/// of any event: they rebuild everything.
fn needs_rebuild(events: &[SignedEvent], decoded: &[Decoded]) -> bool {
    events.iter().any(|e| Op::is_authorization(e.body.kind))
        || decoded
            .iter()
            .any(|d| matches!(d.op, Some(Op::QuarantineResolved(_))))
}

/// Equal bitsets, ignoring trailing zero words.
fn same_bits(a: &Bits, b: &Bits) -> bool {
    let n = a.len().max(b.len());
    (0..n).all(|i| a.get(i).copied().unwrap_or(0) == b.get(i).copied().unwrap_or(0))
}

fn superseded_ids(ctx: &mut Ctx, proj: &Projection) -> BTreeSet<EventHash> {
    crate::conflicts::superseded(ctx, proj)
        .into_iter()
        .map(|i| ctx.graph.events[i].id)
        .collect()
}

/// Merges two sorted lists keeping each event with its decoded content.
fn merge(
    a: Vec<SignedEvent>,
    ad: Vec<Decoded>,
    b: Vec<SignedEvent>,
    bd: Vec<Decoded>,
) -> (Vec<SignedEvent>, Vec<Decoded>) {
    let mut events = Vec::with_capacity(a.len() + b.len());
    let mut decoded = Vec::with_capacity(a.len() + b.len());
    let mut a = a.into_iter().zip(ad).peekable();
    let mut b = b.into_iter().zip(bd).peekable();
    loop {
        let take_a = match (a.peek(), b.peek()) {
            (Some((x, _)), Some((y, _))) => order_key(x) < order_key(y),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let next = if take_a { a.next() } else { b.next() };
        if let Some((e, d)) = next {
            events.push(e);
            decoded.push(d);
        }
    }
    (events, decoded)
}
