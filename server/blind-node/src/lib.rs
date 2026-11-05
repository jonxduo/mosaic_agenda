//! Blind backup node (PROGETTO.md §10.3): a peer without channel keys that
//! keeps every event and attachment of one organization, encrypted, and
//! hands them to the devices that connect later.
//!
//! It never decrypts anything. What it checks is in clear: signatures and
//! ids (by the store), the organization, and the devices allowed to
//! connect, computed from the public device events.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use core_crypto::{ChannelKeyring, DeviceKeys, DevicePublicKey, SecretStore, SecretStoreError};
use core_engine::config::Cfg;
use core_engine::ops::Op;
use core_engine::time::date_of;
use core_events::{DbKey, EventHash, EventKind, EventStore, ObjectType, OrgId, SignedEvent};
use core_model::{Id, PersonId};
use core_sync::blobs::DirBlobStore;
use core_sync::local::LocalLog;
use core_sync::net::{ConfirmFuture, Network, Node, NodeState, PairConfirm, Shared};
use core_sync::pairing::Invitations;
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Config(String),
    #[error("store: {0}")]
    Store(#[from] core_events::StoreError),
    #[error("network: {0}")]
    Sync(#[from] core_sync::SyncError),
    #[error("secrets: {0}")]
    Secrets(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Secrets as files of their own in a directory, readable by the owner
/// only: the node's key and the store's key.
pub struct DirSecrets {
    dir: PathBuf,
}

impl DirSecrets {
    pub fn new(dir: PathBuf) -> Result<Self, Error> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }
}

impl SecretStore for DirSecrets {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        match std::fs::read(self.dir.join(name)) {
            Ok(b) => Ok(Some(Zeroizing::new(b))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(SecretStoreError::Unavailable),
        }
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<(), SecretStoreError> {
        let path = self.dir.join(name);
        std::fs::write(&path, value).map_err(|_| SecretStoreError::Unavailable)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| SecretStoreError::Unavailable)?;
        }
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        match std::fs::remove_file(self.dir.join(name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(SecretStoreError::Unavailable),
        }
    }
}

/// Clear content of a public event, decoded.
fn public_op(e: &SignedEvent) -> Option<Op> {
    if !e.body.kind.is_public() {
        return None;
    }
    let bytes = e.open(&ChannelKeyring::default()).ok()?;
    Op::decode(e.body.kind, &bytes)
}

/// Devices allowed to connect, from the public events alone: the root
/// devices of the genesis, plus devices added by an allowed device, minus
/// the revoked ones. A superset of the exact list (who may add whom is in
/// the encrypted state): a member device could add a device the others
/// would reject. It could only read ciphertext and spend the quota.
/// `None` until the genesis is known.
pub fn authorized(events: &[SignedEvent]) -> Option<BTreeSet<DevicePublicKey>> {
    let mut ordered: Vec<&SignedEvent> = events.iter().collect();
    ordered.sort_by_key(|e| (e.body.depth, e.id));
    let mut genesis = false;
    let mut allowed = BTreeSet::new();
    for e in ordered {
        match (e.body.kind, public_op(e)) {
            (EventKind::OrgGenesis, Some(Op::Genesis(g))) => {
                genesis = true;
                for a in &g.root_admins {
                    allowed.extend(a.devices.iter().map(|d| d.signing));
                }
            }
            (EventKind::DeviceAdded, Some(Op::DeviceAdded(d)))
                if allowed.contains(&e.body.author) =>
            {
                allowed.insert(d.device.signing);
            }
            (EventKind::DeviceRevoked, Some(Op::DeviceRevoked(d)))
                if allowed.contains(&e.body.author) =>
            {
                allowed.remove(&d.device);
            }
            _ => {}
        }
    }
    genesis.then_some(allowed)
}

/// Events whose content the node drops (§14), from what is in clear: the
/// root administrators' erasure and retention events, the object headers
/// and the published configuration.
///
/// - Erasure of a person: the events about the person. Their documents
///   name them only inside the encrypted content, so they stay here as
///   ciphertext; every device that receives them erases them on arrival.
/// - Retention: documents and programs whose last event is older than the
///   retention period of their form or program type. The devices count from
///   the period's last day, which comes before the last event: the node
///   never drops something earlier than they do.
/// - Later events of an object already erased.
pub fn erasure_targets(events: &[SignedEvent]) -> BTreeSet<EventHash> {
    let mut ordered: Vec<&SignedEvent> = events.iter().collect();
    ordered.sort_by_key(|e| (e.body.depth, e.id));
    let mut roots: BTreeSet<PersonId> = BTreeSet::new();
    let mut person_of: BTreeMap<DevicePublicKey, PersonId> = BTreeMap::new();
    let mut cfg: Option<(u32, Cfg)> = None;
    let mut persons: BTreeSet<PersonId> = BTreeSet::new();
    let mut before = None;
    for e in &ordered {
        let root = person_of
            .get(&e.body.author)
            .is_some_and(|p| roots.contains(p));
        match public_op(e) {
            Some(Op::Genesis(g)) => {
                for a in &g.root_admins {
                    roots.insert(a.person);
                    for d in &a.devices {
                        person_of.insert(d.signing, a.person);
                    }
                }
                if let Ok(c) = core_config::decode(&g.config_bundle) {
                    cfg = Some((1, Cfg::new(1, c)));
                }
            }
            Some(Op::DeviceAdded(d)) => {
                person_of.insert(d.device.signing, d.person);
            }
            Some(Op::OrgSettingsChanged(s)) if root => {
                roots.extend(s.add_root_admins.iter().copied());
                for p in &s.remove_root_admins {
                    if roots.len() > 1 {
                        roots.remove(p);
                    }
                }
            }
            Some(Op::ConfigPublished(c)) if cfg.as_ref().is_none_or(|(v, _)| c.version > *v) => {
                if let Ok(compiled) = core_config::decode(&c.bundle) {
                    cfg = Some((c.version, Cfg::new(c.version, compiled)));
                }
            }
            Some(Op::ErasureRequested(r)) if root => {
                persons.insert(r.person);
            }
            Some(Op::RetentionApplied(r)) if root => before = before.max(Some(r.before)),
            _ => {}
        }
    }
    let header = |e: &SignedEvent| e.payload().ok().and_then(|p| p.object);
    let mut objects: BTreeSet<(ObjectType, Id)> = events
        .iter()
        .filter(|e| e.is_erased())
        .filter_map(|e| header(e).map(|h| (h.object, h.id)))
        .collect();
    objects.extend(persons.iter().map(|p| (ObjectType::Person, p.0)));
    if let (Some(before), Some((_, cfg))) = (before, &cfg) {
        // Last event of every document and program, with its form or type.
        let mut last: BTreeMap<(ObjectType, Id), (Option<core_model::Slug>, i64)> = BTreeMap::new();
        for e in events {
            let Some(h) = header(e) else { continue };
            if !matches!(h.object, ObjectType::Document | ObjectType::Program) {
                continue;
            }
            let entry = last.entry((h.object, h.id)).or_insert((None, i64::MIN));
            if entry.0.is_none() {
                entry.0 = h.config.clone();
            }
            entry.1 = entry.1.max(e.body.declared_time);
        }
        for ((object, id), (config, time)) in last {
            let retention = config.as_ref().and_then(|c| match object {
                ObjectType::Document => cfg.form(c).and_then(|f| f.retention),
                _ => cfg.program_type(c).and_then(|t| t.retention),
            });
            let expired = retention
                .and_then(|r| core_engine::engine::expires(date_of(time), r))
                .is_some_and(|end| end < before);
            if expired {
                objects.insert((object, id));
            }
        }
    }
    let programs: Vec<Id> = objects
        .iter()
        .filter(|(t, _)| *t == ObjectType::Program)
        .map(|(_, id)| *id)
        .collect();
    objects.extend(programs.into_iter().map(|id| (ObjectType::Assignment, id)));
    events
        .iter()
        .filter(|e| !e.is_erased())
        .filter(|e| {
            header(e).is_some_and(|h| {
                matches!(
                    h.object,
                    ObjectType::Person
                        | ObjectType::Document
                        | ObjectType::Program
                        | ObjectType::Assignment
                ) && objects.contains(&(h.object, h.id))
            })
        })
        .map(|e| e.id)
        .collect()
}

/// Every attachment of the events still holding their content.
pub fn attachments(events: &[SignedEvent]) -> BTreeSet<[u8; 32]> {
    events
        .iter()
        .filter(|e| !e.is_erased())
        .flat_map(|e| e.body.attachments.iter().map(|b| b.0))
        .collect()
}

/// Bytes used by the files of a directory.
fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|it| {
            it.flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// Never pairs devices: they join through the organization's own devices.
struct NoPairing;

impl PairConfirm for NoPairing {
    fn confirm(&self, _: PersonId, _: core_crypto::DeviceIdentity, _: String) -> ConfirmFuture {
        Box::pin(async { false })
    }
}

pub struct Config {
    /// Holds the store, the attachments and the secrets.
    pub data: PathBuf,
    /// The organization served (its genesis id).
    pub org: OrgId,
    pub network: Network,
    /// Space for attachments; past it new ones are no longer fetched.
    pub quota_bytes: u64,
}

pub struct BlindNode {
    pub node: Arc<Node>,
    state: Shared,
    blobs_dir: PathBuf,
    quota_bytes: u64,
}

impl BlindNode {
    pub async fn start(cfg: Config) -> Result<Self, Error> {
        std::fs::create_dir_all(&cfg.data)?;
        let secrets = DirSecrets::new(cfg.data.join("secrets"))?;
        let keys =
            DeviceKeys::load_or_create(&secrets).map_err(|e| Error::Secrets(e.to_string()))?;
        let db_key = DbKey::load_or_create(&secrets)?;
        let mut store = EventStore::open(&cfg.data.join("events.db"), &db_key)?;
        store.bind_org(cfg.org)?;
        let blobs_dir = cfg.data.join("blobs");
        let blobs = DirBlobStore::new(blobs_dir.clone()).map_err(Error::Config)?;
        let state: Shared = Arc::new(Mutex::new(NodeState {
            store: LocalLog::Open(store),
            blobs: Box::new(blobs),
            wanted_blobs: BTreeSet::new(),
            allowed: None,
            invitations: Invitations::default(),
            paired: Vec::new(),
        }));
        let node = Arc::new(
            Node::spawn(
                Arc::new(keys),
                state.clone(),
                cfg.network,
                Arc::new(NoPairing),
            )
            .await?,
        );
        let me = Self {
            node,
            state,
            blobs_dir,
            quota_bytes: cfg.quota_bytes,
        };
        me.refresh()?;
        Ok(me)
    }

    /// The key the organization's administrator declares (`org.backup_node_set`).
    pub fn id(&self) -> DevicePublicKey {
        self.node.device()
    }

    /// Recomputes who may connect and which attachments to fetch, from
    /// the events received so far.
    pub fn refresh(&self) -> Result<(), Error> {
        let mut st = self
            .state
            .lock()
            .map_err(|_| Error::Config("state poisoned".into()))?;
        let st = &mut *st;
        let LocalLog::Open(store) = &mut st.store else {
            return Ok(());
        };
        // Erasure first (§14): shells instead of contents, attachments gone.
        let targets = erasure_targets(&store.all_events()?);
        for id in &targets {
            if let Some(e) = store.get(id)? {
                for b in &e.body.attachments {
                    let _ = st.blobs.remove(&b.0);
                }
            }
            store.erase(id)?;
        }
        let events = store.all_events()?;
        st.allowed = authorized(&events);
        st.wanted_blobs = if dir_size(&self.blobs_dir) < self.quota_bytes {
            attachments(&events)
        } else {
            BTreeSet::new()
        };
        Ok(())
    }

    /// Every event kept, as stored (erased ones as shells).
    pub fn all_events(&self) -> Result<Vec<SignedEvent>, Error> {
        let st = self
            .state
            .lock()
            .map_err(|_| Error::Config("state poisoned".into()))?;
        match &st.store {
            LocalLog::Open(s) => Ok(s.all_events()?),
            LocalLog::Locked(_) => Ok(Vec::new()),
        }
    }

    /// Number of events kept.
    pub fn events(&self) -> Result<u64, Error> {
        let st = self
            .state
            .lock()
            .map_err(|_| Error::Config("state poisoned".into()))?;
        match &st.store {
            LocalLog::Open(s) => Ok(s.count()?),
            LocalLog::Locked(_) => Ok(0),
        }
    }
}

/// Parses an organization id written as 64 hex digits.
pub fn parse_org(s: &str) -> Result<OrgId, Error> {
    let bytes = hex::decode(s.trim()).map_err(|_| Error::Config("organization: not hex".into()))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Error::Config("organization: 32 bytes expected".into()))?;
    Ok(OrgId(arr))
}
