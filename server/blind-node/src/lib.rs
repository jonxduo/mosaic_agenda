//! Blind backup node (PROGETTO.md §10.3): a peer without channel keys that
//! keeps every event and attachment of one organization, encrypted, and
//! hands them to the devices that connect later.
//!
//! It never decrypts anything. What it checks is in clear: signatures and
//! ids (by the store), the organization, and the devices allowed to
//! connect, computed from the public device events.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use core_crypto::{ChannelKeyring, DeviceKeys, DevicePublicKey, SecretStore, SecretStoreError};
use core_engine::ops::Op;
use core_events::{DbKey, EventKind, EventStore, OrgId, SignedEvent};
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
    fn confirm(
        &self,
        _: core_model::PersonId,
        _: core_crypto::DeviceIdentity,
        _: String,
    ) -> ConfirmFuture {
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
        let events = match &st.store {
            LocalLog::Open(s) => s.all_events()?,
            LocalLog::Locked(_) => return Ok(()),
        };
        st.allowed = authorized(&events);
        st.wanted_blobs = if dir_size(&self.blobs_dir) < self.quota_bytes {
            attachments(&events)
        } else {
            BTreeSet::new()
        };
        Ok(())
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
