//! Local event store: SQLite encrypted with SQLCipher (PROGETTO.md §7.4).
//!
//! Store-level statuses: an event is `pending_parents` until all its parents
//! are present and connected, then `valid` (structurally). The engine (F3)
//! later marks events `invalid`, `pending_key` or `quarantined` after
//! checking permissions. Events with a bad id or signature are never stored.

use std::collections::{BTreeSet, VecDeque};
use std::path::Path;

use core_crypto::local::LocalKey;
use core_crypto::{
    ChannelId, ChannelKey, ChannelKeyring, ChannelRef, DevicePublicKey, SecretStore,
    SecretStoreError, random_array,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use zeroize::Zeroizing;

use crate::MAX_PARENTS;
use crate::error::EventError;
use crate::event::{PayloadChannel, SignedEvent};
use crate::genesis::check_genesis;
use crate::hash::{EventHash, OrgId};

const SCHEMA_VERSION: i64 = 1;
const SECRET_NAME: &str = "store.db_key";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("wrong database key or not a database")]
    WrongKey,
    #[error(transparent)]
    Event(#[from] EventError),
    #[error("event belongs to another organization")]
    WrongOrg,
    #[error("the store is not bound to an organization yet: insert the genesis or bind it")]
    Unbound,
    #[error("corrupt store: {0}")]
    Corrupt(&'static str),
    #[error(transparent)]
    Crypto(#[from] core_crypto::CryptoError),
    #[error(transparent)]
    Secret(#[from] SecretStoreError),
    #[error("status {0:?} cannot be set by hand")]
    BadStatus(EventStatus),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventStatus {
    Valid,
    PendingKey,
    PendingParents,
    Invalid,
    Quarantined,
}

impl EventStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::PendingKey => "pending_key",
            Self::PendingParents => "pending_parents",
            Self::Invalid => "invalid",
            Self::Quarantined => "quarantined",
        }
    }

    fn parse(s: &str) -> Result<Self, StoreError> {
        Ok(match s {
            "valid" => Self::Valid,
            "pending_key" => Self::PendingKey,
            "pending_parents" => Self::PendingParents,
            "invalid" => Self::Invalid,
            "quarantined" => Self::Quarantined,
            _ => return Err(StoreError::Corrupt("unknown status")),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inserted {
    New(EventStatus),
    Duplicate,
}

/// 256-bit random key of the database, kept in the device's secret store.
pub struct DbKey(Zeroizing<[u8; 32]>);

impl DbKey {
    /// Name of the plain key in the secret store (no PIN).
    pub const SECRET: &'static str = SECRET_NAME;

    pub fn generate() -> Result<Self, StoreError> {
        Ok(Self(Zeroizing::new(random_array::<32>()?)))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn load_or_create(store: &dyn SecretStore) -> Result<Self, StoreError> {
        match store.get(SECRET_NAME)? {
            Some(bytes) => {
                let arr: [u8; 32] = bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| SecretStoreError::Corrupt)?;
                Ok(Self::from_bytes(arr))
            }
            None => {
                let key = Self::generate()?;
                store.put(SECRET_NAME, key.0.as_slice())?;
                Ok(key)
            }
        }
    }

    fn hex(&self) -> Zeroizing<String> {
        let mut s = Zeroizing::new(String::with_capacity(64));
        for b in self.0.iter() {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value BLOB NOT NULL);
CREATE TABLE IF NOT EXISTS events (
    id BLOB PRIMARY KEY,
    org BLOB NOT NULL,
    author BLOB NOT NULL,
    depth INTEGER NOT NULL,
    kind TEXT NOT NULL,
    channel BLOB,
    epoch INTEGER,
    raw BLOB NOT NULL,
    received_at INTEGER NOT NULL,
    status TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS events_order ON events(depth, id);
CREATE INDEX IF NOT EXISTS events_status ON events(status);
CREATE TABLE IF NOT EXISTS event_parents (
    child BLOB NOT NULL,
    parent BLOB NOT NULL,
    PRIMARY KEY (child, parent)
);
CREATE INDEX IF NOT EXISTS event_parents_parent ON event_parents(parent);
CREATE TABLE IF NOT EXISTS heads (id BLOB PRIMARY KEY);
CREATE TABLE IF NOT EXISTS channel_keys (
    channel BLOB NOT NULL,
    epoch INTEGER NOT NULL,
    source BLOB NOT NULL,
    wrapped BLOB NOT NULL,
    PRIMARY KEY (channel, epoch, source)
);
CREATE TABLE IF NOT EXISTS blobs (
    hash BLOB PRIMARY KEY,
    size INTEGER NOT NULL,
    state TEXT NOT NULL,
    pinned_reason TEXT
);
CREATE TABLE IF NOT EXISTS receipts (
    event_id BLOB NOT NULL,
    device BLOB NOT NULL,
    received_at INTEGER NOT NULL,
    PRIMARY KEY (event_id, device)
);
CREATE TABLE IF NOT EXISTS outbox (event_id BLOB PRIMARY KEY, created_at INTEGER NOT NULL);
";

fn hash_from(v: Vec<u8>) -> Result<EventHash, StoreError> {
    v.try_into()
        .map(EventHash)
        .map_err(|_| StoreError::Corrupt("hash length"))
}

pub struct EventStore {
    conn: Connection,
    local: LocalKey,
    org: Option<OrgId>,
}

impl EventStore {
    /// Opens (or creates) an encrypted store at `path`.
    pub fn open(path: &Path, key: &DbKey) -> Result<Self, StoreError> {
        Self::setup(Connection::open(path)?, key)
    }

    /// An encrypted in-memory store (tests, simulator).
    pub fn open_in_memory(key: &DbKey) -> Result<Self, StoreError> {
        Self::setup(Connection::open_in_memory()?, key)
    }

    fn setup(conn: Connection, key: &DbKey) -> Result<Self, StoreError> {
        // The hex string is built from bytes, so it cannot inject SQL.
        let pragma = Zeroizing::new(format!("PRAGMA key = \"x'{}'\";", key.hex().as_str()));
        conn.execute_batch(&pragma)?;
        let check: Result<i64, _> =
            conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0));
        match check {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::NotADatabase =>
            {
                return Err(StoreError::WrongKey);
            }
            Err(e) => return Err(e.into()),
        }
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        conn.execute_batch(SCHEMA)?;
        let version: Option<Vec<u8>> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match version {
            None => {
                conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
                    params![SCHEMA_VERSION.to_le_bytes().to_vec()],
                )?;
            }
            Some(v) if v == SCHEMA_VERSION.to_le_bytes() => {}
            Some(_) => return Err(StoreError::Corrupt("unsupported schema version")),
        }
        let org: Option<Vec<u8>> = conn
            .query_row("SELECT value FROM meta WHERE key = 'org'", [], |r| r.get(0))
            .optional()?;
        let org = match org {
            Some(v) => Some(OrgId(
                v.try_into()
                    .map_err(|_| StoreError::Corrupt("org length"))?,
            )),
            None => None,
        };
        let local = LocalKey::derive(&key.0)?;
        Ok(Self { conn, local, org })
    }

    pub fn org(&self) -> Option<OrgId> {
        self.org
    }

    /// Binds an empty store to an organization known from an invitation,
    /// before its genesis arrives.
    pub fn bind_org(&mut self, org: OrgId) -> Result<(), StoreError> {
        match self.org {
            Some(o) if o == org => Ok(()),
            Some(_) => Err(StoreError::WrongOrg),
            None => {
                self.conn.execute(
                    "INSERT INTO meta (key, value) VALUES ('org', ?1)",
                    params![org.0.to_vec()],
                )?;
                self.org = Some(org);
                Ok(())
            }
        }
    }

    /// Inserts a received or locally created event. Verifies it, links it to
    /// its parents and connects any waiting descendants.
    pub fn insert(
        &mut self,
        event: &SignedEvent,
        received_at: i64,
    ) -> Result<Inserted, StoreError> {
        event.verify()?;
        if self.status(&event.id)?.is_some() {
            return Ok(Inserted::Duplicate);
        }
        let org = event.org_id();
        if event.is_genesis() {
            check_genesis(event)?;
        }
        match self.org {
            Some(o) if o != org => return Err(StoreError::WrongOrg),
            Some(_) => {}
            None if event.is_genesis() => self.bind_org(org)?,
            None => return Err(StoreError::Unbound),
        }
        let raw = event.to_bytes()?;
        let (channel, epoch) = match event.body.channel {
            PayloadChannel::Public => (None, None),
            PayloadChannel::Channel(c) => (Some(c.id.0.to_vec()), Some(i64::from(c.epoch))),
        };
        let tx = self.conn.transaction()?;
        let depth = i64::try_from(event.body.depth).map_err(|_| StoreError::Corrupt("depth"))?;
        tx.execute(
            "INSERT INTO events (id, org, author, depth, kind, channel, epoch, raw, received_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                event.id.0.to_vec(),
                org.0.to_vec(),
                event.body.author.0.to_vec(),
                depth,
                event.body.kind.as_str(),
                channel,
                epoch,
                raw,
                received_at,
                EventStatus::PendingParents.as_str(),
            ],
        )?;
        for p in &event.body.parents {
            tx.execute(
                "INSERT INTO event_parents (child, parent) VALUES (?1, ?2)",
                params![event.id.0.to_vec(), p.0.to_vec()],
            )?;
        }
        let status = connect_ready(&tx, event.id)?;
        tx.commit()?;
        Ok(Inserted::New(status))
    }

    pub fn get(&self, id: &EventHash) -> Result<Option<SignedEvent>, StoreError> {
        let raw: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT raw FROM events WHERE id = ?1",
                params![id.0.to_vec()],
                |r| r.get(0),
            )
            .optional()?;
        raw.map(|r| SignedEvent::from_stored_bytes(&r).map_err(StoreError::from))
            .transpose()
    }

    pub fn status(&self, id: &EventHash) -> Result<Option<EventStatus>, StoreError> {
        status_of(&self.conn, id)
    }

    /// Engine-level status changes (never to or from `pending_parents`).
    pub fn set_status(&mut self, id: &EventHash, status: EventStatus) -> Result<(), StoreError> {
        if status == EventStatus::PendingParents {
            return Err(StoreError::BadStatus(status));
        }
        match self.status(id)? {
            None => Err(StoreError::Corrupt("unknown event")),
            Some(EventStatus::PendingParents) => {
                Err(StoreError::BadStatus(EventStatus::PendingParents))
            }
            Some(_) => {
                self.conn.execute(
                    "UPDATE events SET status = ?2 WHERE id = ?1",
                    params![id.0.to_vec(), status.as_str()],
                )?;
                Ok(())
            }
        }
    }

    pub fn count(&self) -> Result<u64, StoreError> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))?;
        u64::try_from(n).map_err(|_| StoreError::Corrupt("count"))
    }

    /// Current heads of the connected graph, ordered.
    pub fn heads(&self) -> Result<Vec<EventHash>, StoreError> {
        let mut stmt = self.conn.prepare("SELECT id FROM heads ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
        rows.map(|r| hash_from(r?)).collect()
    }

    /// Parents for a new local event: up to 16 heads (deepest first, then by
    /// id), with their depths.
    pub fn parents_for_new_event(&self) -> Result<Vec<(EventHash, u64)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT e.id, e.depth FROM heads h JOIN events e ON e.id = h.id ORDER BY e.depth DESC, e.id LIMIT ?1",
        )?;
        let limit = i64::try_from(MAX_PARENTS).map_err(|_| StoreError::Corrupt("limit"))?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?))
        })?;
        rows.map(|r| {
            let (id, depth) = r?;
            Ok((
                hash_from(id)?,
                u64::try_from(depth).map_err(|_| StoreError::Corrupt("depth"))?,
            ))
        })
        .collect()
    }

    /// Parents referenced by stored events but not present: to request from peers.
    pub fn missing(&self) -> Result<Vec<EventHash>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT parent FROM event_parents WHERE parent NOT IN (SELECT id FROM events) ORDER BY parent",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
        rows.map(|r| hash_from(r?)).collect()
    }

    /// Ids with one of the given statuses, in the deterministic order.
    pub fn ordered_ids(&self, statuses: &[EventStatus]) -> Result<Vec<EventHash>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, status FROM events ORDER BY depth, id")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, status) = row?;
            if statuses.contains(&EventStatus::parse(&status)?) {
                out.push(hash_from(id)?);
            }
        }
        Ok(out)
    }

    /// Valid events in the deterministic order.
    pub fn valid_events(&self) -> Result<Vec<SignedEvent>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT raw FROM events WHERE status = 'valid' ORDER BY depth, id")?;
        let rows = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
        rows.map(|r| SignedEvent::from_stored_bytes(&r?).map_err(StoreError::from))
            .collect()
    }

    /// All ancestors of an event (its causal past), excluding itself.
    pub fn ancestors(&self, id: &EventHash) -> Result<BTreeSet<EventHash>, StoreError> {
        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE anc(id) AS (
                 SELECT parent FROM event_parents WHERE child = ?1
                 UNION
                 SELECT p.parent FROM event_parents p JOIN anc ON p.child = anc.id
             ) SELECT id FROM anc",
        )?;
        let rows = stmt.query_map(params![id.0.to_vec()], |r| r.get::<_, Vec<u8>>(0))?;
        rows.map(|r| hash_from(r?)).collect()
    }

    /// True when `ancestor` is in the causal past of `of`.
    pub fn is_ancestor(&self, ancestor: &EventHash, of: &EventHash) -> Result<bool, StoreError> {
        Ok(self.ancestors(of)?.contains(ancestor))
    }

    // ----- synchronization helpers ------------------------------------------

    /// Every stored event id with its depth (any status), ordered by depth then id.
    pub fn ids_with_depth(&self) -> Result<Vec<(EventHash, u64)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, depth FROM events ORDER BY depth, id")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?)))?;
        rows.map(|r| {
            let (id, d) = r?;
            Ok((
                hash_from(id)?,
                u64::try_from(d).map_err(|_| StoreError::Corrupt("depth"))?,
            ))
        })
        .collect()
    }

    /// Serialized event, as received.
    pub fn raw(&self, id: &EventHash) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT raw FROM events WHERE id = ?1",
                params![id.0.to_vec()],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Every stored event, any status, in the deterministic order.
    pub fn all_events(&self) -> Result<Vec<SignedEvent>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT raw FROM events ORDER BY depth, id")?;
        let rows = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
        rows.map(|r| SignedEvent::from_stored_bytes(&r?).map_err(StoreError::from))
            .collect()
    }

    /// Events stored after the row `after` (0 = all), with the last row
    /// read: lets a reader keep up with the store without rereading it.
    /// Events are never deleted, so rows only grow.
    pub fn events_since(&self, after: i64) -> Result<(Vec<SignedEvent>, i64), StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT rowid, raw FROM events WHERE rowid > ?1 ORDER BY rowid")?;
        let rows = stmt.query_map(params![after], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        let mut last = after;
        let mut out = Vec::new();
        for r in rows {
            let (row, raw) = r?;
            last = last.max(row);
            out.push(SignedEvent::from_stored_bytes(&raw)?);
        }
        Ok((out, last))
    }

    /// Records the state of an attachment blob.
    pub fn set_blob(
        &mut self,
        hash: &[u8; 32],
        size: u64,
        state: &str,
        pinned_reason: Option<&str>,
    ) -> Result<(), StoreError> {
        let size = i64::try_from(size).map_err(|_| StoreError::Corrupt("blob size"))?;
        self.conn.execute(
            "INSERT INTO blobs (hash, size, state, pinned_reason) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(hash) DO UPDATE SET size = excluded.size, state = excluded.state, pinned_reason = excluded.pinned_reason",
            params![hash.to_vec(), size, state, pinned_reason],
        )?;
        Ok(())
    }

    pub fn blob_state(&self, hash: &[u8; 32]) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT state FROM blobs WHERE hash = ?1",
                params![hash.to_vec()],
                |r| r.get(0),
            )
            .optional()?)
    }

    // ----- channel keys ----------------------------------------------------

    fn key_context(channel: &ChannelRef, source: &EventHash) -> Vec<u8> {
        let mut c = Vec::with_capacity(68);
        c.extend_from_slice(&channel.id.0);
        c.extend_from_slice(&channel.epoch.to_le_bytes());
        c.extend_from_slice(&source.0);
        c
    }

    /// Stores a channel key, wrapped with the local key.
    pub fn put_channel_key(
        &mut self,
        channel: ChannelRef,
        source: EventHash,
        key: &ChannelKey,
    ) -> Result<(), StoreError> {
        let wrapped = self
            .local
            .wrap(&Self::key_context(&channel, &source), key.as_bytes())?;
        self.conn.execute(
            "INSERT OR IGNORE INTO channel_keys (channel, epoch, source, wrapped) VALUES (?1, ?2, ?3, ?4)",
            params![channel.id.0.to_vec(), i64::from(channel.epoch), source.0.to_vec(), wrapped],
        )?;
        Ok(())
    }

    pub fn keyring(&self) -> Result<ChannelKeyring, StoreError> {
        let mut ring = ChannelKeyring::new();
        let mut stmt = self
            .conn
            .prepare("SELECT channel, epoch, source, wrapped FROM channel_keys")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })?;
        for row in rows {
            let (channel, epoch, source, wrapped) = row?;
            let channel = ChannelRef {
                id: ChannelId(
                    channel
                        .try_into()
                        .map_err(|_| StoreError::Corrupt("channel length"))?,
                ),
                epoch: u32::try_from(epoch).map_err(|_| StoreError::Corrupt("epoch"))?,
            };
            let source = hash_from(source)?;
            let secret = self
                .local
                .unwrap_secret(&Self::key_context(&channel, &source), &wrapped)?;
            let bytes: [u8; 32] = secret
                .as_slice()
                .try_into()
                .map_err(|_| StoreError::Corrupt("key length"))?;
            ring.insert(channel, source.0, ChannelKey::from_bytes(bytes));
        }
        Ok(ring)
    }

    // ----- outbox and receipts ---------------------------------------------

    pub fn outbox_add(&mut self, id: &EventHash, created_at: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO outbox (event_id, created_at) VALUES (?1, ?2)",
            params![id.0.to_vec(), created_at],
        )?;
        Ok(())
    }

    pub fn outbox_remove(&mut self, id: &EventHash) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM outbox WHERE event_id = ?1",
            params![id.0.to_vec()],
        )?;
        Ok(())
    }

    pub fn outbox(&self) -> Result<Vec<EventHash>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT event_id FROM outbox ORDER BY created_at, event_id")?;
        let rows = stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
        rows.map(|r| hash_from(r?)).collect()
    }

    /// Records a delivery receipt (idempotent).
    pub fn add_receipt(
        &mut self,
        id: &EventHash,
        device: &DevicePublicKey,
        received_at: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO receipts (event_id, device, received_at) VALUES (?1, ?2, ?3)",
            params![id.0.to_vec(), device.0.to_vec(), received_at],
        )?;
        Ok(())
    }

    pub fn receipts(&self, id: &EventHash) -> Result<BTreeSet<DevicePublicKey>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT device FROM receipts WHERE event_id = ?1")?;
        let rows = stmt.query_map(params![id.0.to_vec()], |r| r.get::<_, Vec<u8>>(0))?;
        rows.map(|r| {
            r?.try_into()
                .map(DevicePublicKey)
                .map_err(|_| StoreError::Corrupt("device length"))
        })
        .collect()
    }
}

fn status_of(conn: &Connection, id: &EventHash) -> Result<Option<EventStatus>, StoreError> {
    let s: Option<String> = conn
        .query_row(
            "SELECT status FROM events WHERE id = ?1",
            params![id.0.to_vec()],
            |r| r.get(0),
        )
        .optional()?;
    s.map(|s| EventStatus::parse(&s)).transpose()
}

fn parents_of(tx: &Transaction<'_>, id: &EventHash) -> Result<Vec<EventHash>, StoreError> {
    let mut stmt = tx.prepare("SELECT parent FROM event_parents WHERE child = ?1")?;
    let rows = stmt.query_map(params![id.0.to_vec()], |r| r.get::<_, Vec<u8>>(0))?;
    rows.map(|r| hash_from(r?)).collect()
}

fn children_of(tx: &Transaction<'_>, id: &EventHash) -> Result<Vec<EventHash>, StoreError> {
    let mut stmt =
        tx.prepare("SELECT child FROM event_parents WHERE parent = ?1 ORDER BY child")?;
    let rows = stmt.query_map(params![id.0.to_vec()], |r| r.get::<_, Vec<u8>>(0))?;
    rows.map(|r| hash_from(r?)).collect()
}

fn depth_of(tx: &Transaction<'_>, id: &EventHash) -> Result<Option<i64>, StoreError> {
    Ok(tx
        .query_row(
            "SELECT depth FROM events WHERE id = ?1",
            params![id.0.to_vec()],
            |r| r.get(0),
        )
        .optional()?)
}

fn is_connected(status: Option<EventStatus>) -> bool {
    matches!(status, Some(s) if s != EventStatus::PendingParents)
}

/// If `start` has all its parents connected, connects it (status valid, or
/// invalid on a depth mismatch) and then every waiting descendant that
/// becomes ready. Returns the resulting status of `start`.
fn connect_ready(tx: &Transaction<'_>, start: EventHash) -> Result<EventStatus, StoreError> {
    let mut queue = VecDeque::from([start]);
    let mut result = EventStatus::PendingParents;
    while let Some(id) = queue.pop_front() {
        if status_of(tx, &id)? != Some(EventStatus::PendingParents) {
            continue;
        }
        let parents = parents_of(tx, &id)?;
        let mut max_parent_depth: Option<i64> = None;
        let mut ready = true;
        for p in &parents {
            if !is_connected(status_of(tx, p)?) {
                ready = false;
                break;
            }
            let d = depth_of(tx, p)?.ok_or(StoreError::Corrupt("parent depth"))?;
            max_parent_depth = Some(max_parent_depth.map_or(d, |m: i64| m.max(d)));
        }
        if !ready {
            continue;
        }
        let depth = depth_of(tx, &id)?.ok_or(StoreError::Corrupt("depth"))?;
        let expected = max_parent_depth.map_or(0, |d| d + 1);
        let status = if depth == expected {
            EventStatus::Valid
        } else {
            EventStatus::Invalid
        };
        tx.execute(
            "UPDATE events SET status = ?2 WHERE id = ?1",
            params![id.0.to_vec(), status.as_str()],
        )?;
        for p in &parents {
            tx.execute("DELETE FROM heads WHERE id = ?1", params![p.0.to_vec()])?;
        }
        let children = children_of(tx, &id)?;
        let mut has_connected_child = false;
        for c in &children {
            if is_connected(status_of(tx, c)?) {
                has_connected_child = true;
            }
        }
        if !has_connected_child {
            tx.execute(
                "INSERT OR IGNORE INTO heads (id) VALUES (?1)",
                params![id.0.to_vec()],
            )?;
        }
        if id == start {
            result = status;
        }
        queue.extend(children);
    }
    Ok(result)
}
