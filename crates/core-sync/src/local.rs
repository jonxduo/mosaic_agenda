//! What sync works on: the encrypted store, or — while the app is locked
//! with its PIN — an inbox that only receives (PROGETTO.md §8.6).
//!
//! The inbox knows the ids and depths of the events already stored (written
//! when the app locks), so peers do not resend them. Events received while
//! locked are sealed to the [`InboxPublicKey`] and written one file each;
//! only the database key (unlocked with the PIN) can open them. Ids and
//! depths stay readable: they reveal no content, author or kind.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use core_crypto::DevicePublicKey;
use core_crypto::pin::{InboxKey, InboxPublicKey};
use core_events::{EventHash, EventStatus, EventStore, Inserted, OrgId, SignedEvent, StoreError};
use serde::{Deserialize, Serialize};

const INDEX: &str = "index.bin";
const SUFFIX: &str = ".sealed";

/// The local log as sync sees it.
pub enum LocalLog {
    Open(EventStore),
    Locked(Inbox),
}

impl LocalLog {
    pub fn is_locked(&self) -> bool {
        matches!(self, Self::Locked(_))
    }

    pub fn store(&self) -> Option<&EventStore> {
        match self {
            Self::Open(s) => Some(s),
            Self::Locked(_) => None,
        }
    }

    pub fn store_mut(&mut self) -> Option<&mut EventStore> {
        match self {
            Self::Open(s) => Some(s),
            Self::Locked(_) => None,
        }
    }

    pub fn org(&self) -> Option<OrgId> {
        match self {
            Self::Open(s) => s.org(),
            Self::Locked(i) => Some(i.org),
        }
    }

    pub fn ids_with_depth(&self) -> Result<Vec<(EventHash, u64)>, StoreError> {
        match self {
            Self::Open(s) => s.ids_with_depth(),
            Self::Locked(i) => Ok(i.ids_with_depth()),
        }
    }

    /// Bytes of an event to send; the inbox cannot send anything.
    pub fn raw(&self, id: &EventHash) -> Result<Option<Vec<u8>>, StoreError> {
        match self {
            Self::Open(s) => s.raw(id),
            Self::Locked(_) => Ok(None),
        }
    }

    pub fn status(&self, id: &EventHash) -> Result<Option<EventStatus>, StoreError> {
        match self {
            Self::Open(s) => s.status(id),
            Self::Locked(i) => Ok(i.contains(id).then_some(EventStatus::Valid)),
        }
    }

    pub fn insert(
        &mut self,
        event: &SignedEvent,
        received_at: i64,
    ) -> Result<Inserted, StoreError> {
        match self {
            Self::Open(s) => s.insert(event, received_at),
            Self::Locked(i) => i.insert(event),
        }
    }

    pub fn missing(&self) -> Result<Vec<EventHash>, StoreError> {
        match self {
            Self::Open(s) => s.missing(),
            Self::Locked(i) => Ok(i.missing()),
        }
    }

    /// Receipts are dropped while locked (the peer sends them again).
    pub fn add_receipt(
        &mut self,
        event: &EventHash,
        device: &DevicePublicKey,
        at: i64,
    ) -> Result<(), StoreError> {
        match self {
            Self::Open(s) => s.add_receipt(event, device, at),
            Self::Locked(_) => Ok(()),
        }
    }

    pub fn set_blob(
        &mut self,
        hash: &[u8; 32],
        size: u64,
        state: &str,
        pinned: Option<&str>,
    ) -> Result<(), StoreError> {
        match self {
            Self::Open(s) => s.set_blob(hash, size, state, pinned),
            Self::Locked(_) => Ok(()),
        }
    }

    pub fn bind_org(&mut self, org: OrgId) -> Result<(), StoreError> {
        match self {
            Self::Open(s) => s.bind_org(org),
            Self::Locked(_) => Err(StoreError::Corrupt("the app is locked")),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Index {
    org: OrgId,
    seal_to: InboxPublicKey,
    known: Vec<(EventHash, u64)>,
}

/// Receive-only log used while the app is locked.
pub struct Inbox {
    dir: PathBuf,
    org: OrgId,
    seal_to: InboxPublicKey,
    known: BTreeMap<EventHash, u64>,
    /// Sealed events waiting for the unlock: id → (depth, parents).
    received: BTreeMap<EventHash, (u64, Vec<EventHash>)>,
}

fn file_name(depth: u64, id: &EventHash) -> String {
    let hex: String = id.0.iter().map(|b| format!("{b:02x}")).collect();
    format!("{depth:020}-{hex}{SUFFIX}")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

impl Inbox {
    /// Records what the store holds (at lock time) and opens the inbox.
    pub fn create(
        dir: &Path,
        store: &EventStore,
        seal_to: InboxPublicKey,
    ) -> Result<Self, StoreError> {
        let org = store.org().ok_or(StoreError::Unbound)?;
        std::fs::create_dir_all(dir)?;
        let index = Index {
            org,
            seal_to,
            known: store.ids_with_depth()?,
        };
        let bytes =
            postcard::to_allocvec(&index).map_err(|_| StoreError::Corrupt("inbox index"))?;
        write_atomic(&dir.join(INDEX), &bytes)?;
        Self::open(dir)?.ok_or(StoreError::Corrupt("inbox index"))
    }

    /// Reopens the inbox after a restart while locked (`None` = never created).
    pub fn open(dir: &Path) -> Result<Option<Self>, StoreError> {
        let Ok(bytes) = std::fs::read(dir.join(INDEX)) else {
            return Ok(None);
        };
        let index: Index =
            postcard::from_bytes(&bytes).map_err(|_| StoreError::Corrupt("inbox index"))?;
        let mut received = BTreeMap::new();
        for entry in std::fs::read_dir(dir)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(SUFFIX) else {
                continue;
            };
            let Some((depth, hex)) = stem.split_once('-') else {
                continue;
            };
            let (Ok(depth), Some(id)) = (depth.parse::<u64>(), parse_hex(hex)) else {
                continue;
            };
            received.insert(id, (depth, Vec::new()));
        }
        Ok(Some(Self {
            dir: dir.to_path_buf(),
            org: index.org,
            seal_to: index.seal_to,
            known: index.known.into_iter().collect(),
            received,
        }))
    }

    pub fn contains(&self, id: &EventHash) -> bool {
        self.known.contains_key(id) || self.received.contains_key(id)
    }

    /// Key the received events are sealed to.
    pub fn seal_to(&self) -> InboxPublicKey {
        self.seal_to
    }

    /// Events waiting for the unlock.
    pub fn pending(&self) -> usize {
        self.received.len()
    }

    fn ids_with_depth(&self) -> Vec<(EventHash, u64)> {
        let mut out: Vec<(EventHash, u64)> = self
            .known
            .iter()
            .map(|(id, d)| (*id, *d))
            .chain(self.received.iter().map(|(id, (d, _))| (*id, *d)))
            .collect();
        out.sort_by_key(|(id, d)| (*d, *id));
        out
    }

    /// Parents of received events that are nowhere yet.
    fn missing(&self) -> Vec<EventHash> {
        let set: BTreeSet<EventHash> = self
            .received
            .values()
            .flat_map(|(_, parents)| parents.iter().copied())
            .filter(|p| !self.contains(p))
            .collect();
        set.into_iter().collect()
    }

    fn insert(&mut self, event: &SignedEvent) -> Result<Inserted, StoreError> {
        event.verify()?;
        if event.org_id() != self.org {
            return Err(StoreError::WrongOrg);
        }
        if self.contains(&event.id) {
            return Ok(Inserted::Duplicate);
        }
        let sealed = self.seal_to.seal(&event.to_bytes()?)?;
        write_atomic(
            &self.dir.join(file_name(event.body.depth, &event.id)),
            &sealed,
        )?;
        self.received
            .insert(event.id, (event.body.depth, event.body.parents.clone()));
        Ok(Inserted::New(EventStatus::PendingParents))
    }

    /// Moves the received events into the store (oldest first) and deletes
    /// their files; the index stays for a later restart while locked.
    /// Returns how many were new to the store.
    pub fn drain_into(
        self,
        store: &mut EventStore,
        key: &InboxKey,
        now: i64,
    ) -> Result<usize, StoreError> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().ends_with(SUFFIX))
            .collect();
        files.sort();
        let mut added = 0;
        for f in &files {
            let sealed = std::fs::read(f)?;
            // A file that does not open is dropped: the peer will send the
            // event again.
            let Ok(raw) = key.open(&sealed) else { continue };
            let Ok(event) = SignedEvent::from_bytes(&raw) else {
                continue;
            };
            match store.insert(&event, now) {
                Ok(Inserted::New(_)) => added += 1,
                Ok(Inserted::Duplicate) | Err(StoreError::WrongOrg | StoreError::Event(_)) => {}
                Err(e) => return Err(e),
            }
        }
        for f in &files {
            std::fs::remove_file(f)?;
        }
        Ok(added)
    }
}

fn parse_hex(s: &str) -> Option<EventHash> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(EventHash(out))
}

/// Removes a stale inbox folder (e.g. after the PIN is removed).
pub fn discard(dir: &Path) -> Result<(), StoreError> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}
