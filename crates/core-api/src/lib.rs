//! Facade for the apps (PROGETTO.md §12): one [`Core`] per device, with
//! typed commands returning DTOs whose labels are already resolved.
//!
//! The desktop and mobile apps expose these commands through Tauri; the
//! development HTTP bridge (`api-dev-server`) exposes the same ones to the
//! browser and to the Playwright tests.

pub mod commands;
pub mod dispatch;
pub mod dto;
pub mod push;
pub mod secrets;
pub mod texts;
pub mod views;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use core_config::CompiledConfig;
use core_crypto::pin::{InboxKey, InboxPublicKey, PinParams, PinWrap};
use core_crypto::{
    ChannelKeyring, CryptoError, DeviceIdentity, DeviceKeys, DevicePublicKey, SecretStore,
};
use core_engine::actions::{ActionError, Client};
use core_engine::{Engine, Projection};
use core_events::{DbKey, EventStore, SignedEvent};
use core_model::{Id, PersonId};
use core_sync::blobs::DirBlobStore;
use core_sync::local::{Inbox, LocalLog};
use core_sync::net::{ConfirmFuture, Network, Node, NodeState, PairConfirm, Shared};
use core_sync::pairing::Invitations;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::oneshot;

pub use core_events::payloads::NetworkParams;
pub use dto::*;
use texts::Texts;

#[derive(Debug, Clone, Serialize, Deserialize, Type, thiserror::Error)]
#[error("{kind}: {message}")]
pub struct ApiError {
    /// `denied`, `not_found`, `invalid`, `locked`, `network`, `internal`.
    pub kind: String,
    pub message: String,
}

impl ApiError {
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.to_owned(),
            message: message.into(),
        }
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::new("internal", e.to_string())
    }
    pub fn invalid(e: impl std::fmt::Display) -> Self {
        Self::new("invalid", e.to_string())
    }
    pub fn not_found(what: impl std::fmt::Display) -> Self {
        Self::new("not_found", what.to_string())
    }
}

impl From<ActionError> for ApiError {
    fn from(e: ActionError) -> Self {
        match e {
            ActionError::Denied(m) => Self::new("denied", m),
            ActionError::NotFound(m) => Self::not_found(m),
            other => Self::internal(other),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// How a core is created.
pub struct CoreOptions {
    pub data_dir: PathBuf,
    /// Compiled package embedded in the app (initial configuration).
    pub bundle: Vec<u8>,
    pub secrets: Arc<dyn SecretStore>,
    /// `None` = offline (tests, development without network).
    pub network: Option<Network>,
    /// Fixed clock (tests): milliseconds since the Unix epoch.
    pub clock: Option<i64>,
    /// Cost of the PIN key derivation (`None` = the secure default).
    pub pin_params: Option<PinParams>,
    /// Server written in the genesis of an organization created here
    /// (relays, discovery, push); `None` = development defaults.
    pub server: Option<core_events::payloads::NetworkParams>,
}

/// A decoded view of the local events, rebuilt when they change.
pub struct View {
    pub events: Vec<SignedEvent>,
    pub proj: Projection,
    pub ring: ChannelKeyring,
    /// Engine the view comes from (dry run of actions).
    pub engine: Arc<Mutex<Engine>>,
}

struct PendingPairing {
    person: PersonId,
    identity: DeviceIdentity,
    code: String,
    decide: Option<oneshot::Sender<bool>>,
}

#[derive(Default)]
struct Pairings {
    pending: BTreeMap<String, PendingPairing>,
}

struct UiConfirm {
    pairings: Arc<Mutex<Pairings>>,
}

impl PairConfirm for UiConfirm {
    fn confirm(&self, person: PersonId, identity: DeviceIdentity, code: String) -> ConfirmFuture {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut p) = self.pairings.lock() {
            p.pending.insert(
                code.clone(),
                PendingPairing {
                    person,
                    identity,
                    code,
                    decide: Some(tx),
                },
            );
        }
        Box::pin(async move {
            matches!(
                tokio::time::timeout(std::time::Duration::from_secs(300), rx).await,
                Ok(Ok(true))
            )
        })
    }
}

/// Settings kept on the device only (never shared).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalSettings {
    pub auto_lock_minutes: Option<u32>,
    pub mobile_data_photos: bool,
    pub muted_categories: Vec<String>,
    pub privacy_accepted: bool,
    /// Notifications already shown (trigger event + text).
    pub notified: Vec<String>,
    pub last_sync: Option<i64>,
    /// Fingerprint of the membership last sent to the push service.
    #[serde(default)]
    pub push_published: Option<String>,
}

pub struct Core {
    pub data_dir: PathBuf,
    pub bundle: Vec<u8>,
    pub package: CompiledConfig,
    pub keys: Arc<DeviceKeys>,
    secrets: Arc<dyn SecretStore>,
    pub state: Shared,
    node: tokio::sync::Mutex<Option<Arc<Node>>>,
    network: Option<Network>,
    cache: Mutex<Option<Arc<View>>>,
    /// Bumped by every change: a view computed meanwhile is not cached.
    generation: AtomicU64,
    /// Incremental engine and the last store row it has seen.
    engine: Mutex<Option<(Arc<Mutex<Engine>>, i64)>>,
    /// Database key while unlocked (`None` while locked with the PIN).
    db_key: Mutex<Option<DbKey>>,
    /// Where events received while locked are sealed to.
    inbox_public: Mutex<Option<InboxPublicKey>>,
    pin_params: PinParams,
    pub(crate) server: Option<core_events::payloads::NetworkParams>,
    /// Push service client, by URL.
    push: Mutex<Option<(String, core_push::PushClient)>>,
    /// Last wake-up request per device (client-side coalescing).
    woken: Mutex<BTreeMap<DevicePublicKey, i64>>,
    /// Peers of the last unlocked round, to keep receiving while locked.
    peers_cache: Mutex<Vec<DevicePublicKey>>,
    pairings: Arc<Mutex<Pairings>>,
    local: Mutex<LocalSettings>,
    locked: AtomicBool,
    sync_scheduled: AtomicBool,
    clock: Option<i64>,
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> ApiResult<std::sync::MutexGuard<'_, T>> {
    m.lock().map_err(|_| ApiError::internal("state poisoned"))
}

/// Database key wrapped with the PIN (replaces the plain key when a PIN is set).
const DB_KEY_PIN: &str = "store.db_key.pin";
/// Random pepper of the PIN key derivation, kept in the system keychain.
const PIN_PEPPER: &str = "app.pin.pepper";

pub(crate) fn locked_error() -> ApiError {
    ApiError::new("locked", "the app is locked")
}

impl Core {
    pub fn open(opts: CoreOptions) -> ApiResult<Arc<Self>> {
        std::fs::create_dir_all(&opts.data_dir).map_err(ApiError::internal)?;
        let package = core_config::decode(&opts.bundle)
            .map_err(|e| ApiError::internal(format!("package: {e}")))?;
        let keys = Arc::new(
            DeviceKeys::load_or_create(opts.secrets.as_ref()).map_err(ApiError::internal)?,
        );
        let with_pin = opts
            .secrets
            .get(DB_KEY_PIN)
            .map_err(ApiError::internal)?
            .is_some();
        let (log, db_key, inbox_public) = if with_pin {
            // Locked until the PIN is entered: only the sealed inbox is open.
            let inbox = Inbox::open(&opts.data_dir.join("inbox"))
                .map_err(ApiError::internal)?
                .ok_or_else(|| ApiError::internal("PIN set but the inbox index is missing"))?;
            let public = inbox.seal_to();
            (LocalLog::Locked(inbox), None, Some(public))
        } else {
            let key = DbKey::load_or_create(opts.secrets.as_ref()).map_err(ApiError::internal)?;
            let store = EventStore::open(&opts.data_dir.join("events.db"), &key)
                .map_err(ApiError::internal)?;
            let public = InboxKey::derive(key.as_bytes()).public();
            (LocalLog::Open(store), Some(key), Some(public))
        };
        let blobs = DirBlobStore::new(opts.data_dir.join("blobs")).map_err(ApiError::internal)?;
        let local: LocalSettings = std::fs::read(opts.data_dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let state = Arc::new(Mutex::new(NodeState {
            store: log,
            blobs: Box::new(blobs),
            wanted_blobs: Default::default(),
            // After a restart while locked nobody is known yet: no sync
            // until the first unlock.
            allowed: with_pin.then(Default::default),
            invitations: Invitations::default(),
            paired: Vec::new(),
        }));
        Ok(Arc::new(Self {
            data_dir: opts.data_dir,
            bundle: opts.bundle,
            package,
            keys,
            secrets: opts.secrets,
            state,
            node: tokio::sync::Mutex::new(None),
            network: opts.network,
            cache: Mutex::new(None),
            generation: AtomicU64::new(0),
            engine: Mutex::new(None),
            db_key: Mutex::new(db_key),
            inbox_public: Mutex::new(inbox_public),
            pin_params: opts.pin_params.unwrap_or_default(),
            server: opts.server,
            peers_cache: Mutex::new(Vec::new()),
            push: Mutex::new(None),
            woken: Mutex::new(BTreeMap::new()),
            pairings: Arc::new(Mutex::new(Pairings::default())),
            local: Mutex::new(local),
            locked: AtomicBool::new(with_pin),
            sync_scheduled: AtomicBool::new(false),
            clock: opts.clock,
        }))
    }

    pub fn now(&self) -> i64 {
        self.clock.unwrap_or_else(now_ms)
    }

    pub fn texts(&self) -> Texts {
        let pkg = &self.package.package;
        let locale = pkg.manifest.default_locale.to_string();
        let view_cfg = self.cache.lock().ok().and_then(|c| {
            c.as_ref()
                .and_then(|v| v.proj.cfg().map(|c| c.compiled.clone()))
        });
        let texts = view_cfg.as_ref().map_or(&pkg.texts, |c| &c.package.texts);
        Texts::new(&locale, texts.get(&pkg.manifest.default_locale))
    }

    pub fn has_org(&self) -> bool {
        self.state
            .lock()
            .map(|s| s.store.org().is_some())
            .unwrap_or(false)
    }

    fn save_local(&self) -> ApiResult<()> {
        let local = lock(&self.local)?.clone();
        let bytes = serde_json::to_vec_pretty(&local).map_err(ApiError::internal)?;
        std::fs::write(self.data_dir.join("settings.json"), bytes).map_err(ApiError::internal)
    }

    pub(crate) fn local(&self) -> ApiResult<LocalSettings> {
        Ok(lock(&self.local)?.clone())
    }

    pub(crate) fn update_local(&self, f: impl FnOnce(&mut LocalSettings)) -> ApiResult<()> {
        f(&mut *lock(&self.local)?);
        self.save_local()
    }

    // ----- lock -----------------------------------------------------------------

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::SeqCst)
    }

    pub fn has_pin(&self) -> bool {
        self.secrets.get(DB_KEY_PIN).ok().flatten().is_some()
    }

    fn inbox_dir(&self) -> PathBuf {
        self.data_dir.join("inbox")
    }

    /// Sets, changes or removes the PIN. With a PIN the database key is
    /// stored only wrapped with it (§8.6); the app must be unlocked.
    pub fn set_pin(&self, pin: Option<String>) -> ApiResult<()> {
        self.ensure_unlocked()?;
        let key = zeroize::Zeroizing::new(
            *lock(&self.db_key)?
                .as_ref()
                .ok_or_else(locked_error)?
                .as_bytes(),
        );
        match pin {
            Some(p) if p.chars().count() >= 4 => {
                if !self.has_org() {
                    return Err(ApiError::invalid("create or join the organization first"));
                }
                let pepper = match self.secrets.get(PIN_PEPPER).map_err(ApiError::internal)? {
                    Some(b) if b.len() == 32 => {
                        let mut a = [0u8; 32];
                        a.copy_from_slice(&b);
                        a
                    }
                    _ => {
                        let a = core_crypto::random_array::<32>().map_err(ApiError::internal)?;
                        self.secrets
                            .put(PIN_PEPPER, &a)
                            .map_err(ApiError::internal)?;
                        a
                    }
                };
                let wrap = PinWrap::seal(&p, &pepper, &key, self.pin_params)
                    .map_err(ApiError::internal)?;
                // Index for a restart while locked.
                let public = InboxKey::derive(&key).public();
                {
                    let st = lock(&self.state)?;
                    let store = st.store.store().ok_or_else(locked_error)?;
                    Inbox::create(&self.inbox_dir(), store, public).map_err(ApiError::internal)?;
                }
                // Wrapped key first, then the plain one goes.
                self.secrets
                    .put(DB_KEY_PIN, &wrap.to_bytes().map_err(ApiError::internal)?)
                    .map_err(ApiError::internal)?;
                self.secrets
                    .delete(DbKey::SECRET)
                    .map_err(ApiError::internal)
            }
            Some(_) => Err(ApiError::invalid("the PIN needs at least 4 digits")),
            None => {
                self.secrets
                    .put(DbKey::SECRET, key.as_slice())
                    .map_err(ApiError::internal)?;
                self.secrets
                    .delete(DB_KEY_PIN)
                    .map_err(ApiError::internal)?;
                self.secrets
                    .delete(PIN_PEPPER)
                    .map_err(ApiError::internal)?;
                core_sync::local::discard(&self.inbox_dir()).map_err(ApiError::internal)
            }
        }
    }

    /// Opens the database with the PIN and moves in what arrived while locked.
    /// `Ok(false)` for a wrong PIN.
    pub fn unlock(&self, pin: &str) -> ApiResult<bool> {
        if !self.is_locked() {
            return Ok(true);
        }
        let Some(wrapped) = self.secrets.get(DB_KEY_PIN).map_err(ApiError::internal)? else {
            self.locked.store(false, Ordering::SeqCst);
            return Ok(true);
        };
        let pepper: [u8; 32] = self
            .secrets
            .get(PIN_PEPPER)
            .map_err(ApiError::internal)?
            .and_then(|b| b.as_slice().try_into().ok())
            .ok_or_else(|| ApiError::internal("PIN pepper missing"))?;
        let wrap = PinWrap::from_bytes(&wrapped).map_err(ApiError::internal)?;
        let secret = match wrap.open(pin, &pepper) {
            Ok(s) => s,
            Err(CryptoError::Decrypt) => return Ok(false),
            Err(e) => return Err(ApiError::internal(e)),
        };
        let key = DbKey::from_bytes(*secret);
        let store =
            EventStore::open(&self.data_dir.join("events.db"), &key).map_err(ApiError::internal)?;
        let inbox_key = InboxKey::derive(key.as_bytes());
        {
            let mut st = lock(&self.state)?;
            let previous = std::mem::replace(&mut st.store, LocalLog::Open(store));
            if let (LocalLog::Locked(inbox), Some(store)) = (previous, st.store.store_mut()) {
                inbox
                    .drain_into(store, &inbox_key, self.now())
                    .map_err(ApiError::internal)?;
                // Fresh index, for a restart while locked.
                Inbox::create(&self.inbox_dir(), store, inbox_key.public())
                    .map_err(ApiError::internal)?;
            }
        }
        *lock(&self.db_key)? = Some(key);
        *lock(&self.inbox_public)? = Some(inbox_key.public());
        self.invalidate();
        self.locked.store(false, Ordering::SeqCst);
        Ok(true)
    }

    /// Locks now if a PIN is set: closes the database and forgets the key
    /// and everything decrypted; sync keeps receiving into the inbox.
    pub fn lock_now(&self) {
        if self.has_pin() {
            let _ = self.close_store();
        }
    }

    fn close_store(&self) -> ApiResult<()> {
        let public =
            (*lock(&self.inbox_public)?).ok_or_else(|| ApiError::internal("no inbox key"))?;
        {
            let mut st = lock(&self.state)?;
            if let LocalLog::Open(store) = &st.store {
                let inbox =
                    Inbox::create(&self.inbox_dir(), store, public).map_err(ApiError::internal)?;
                st.store = LocalLog::Locked(inbox);
            }
        }
        self.locked.store(true, Ordering::SeqCst);
        *lock(&self.db_key)? = None;
        self.invalidate();
        *lock(&self.engine)? = None;
        Ok(())
    }

    /// Events received while locked (for a generic notification).
    pub fn pending_while_locked(&self) -> usize {
        self.state
            .lock()
            .ok()
            .and_then(|st| match &st.store {
                LocalLog::Locked(i) => Some(i.pending()),
                LocalLog::Open(_) => None,
            })
            .unwrap_or(0)
    }

    pub(crate) fn ensure_unlocked(&self) -> ApiResult<()> {
        if self.is_locked() {
            Err(ApiError::new("locked", "the app is locked"))
        } else {
            Ok(())
        }
    }

    // ----- events -----------------------------------------------------------------

    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut c) = self.cache.lock() {
            *c = None;
        }
    }

    /// Current view of the local events (cached). After a change only the
    /// new events are read from the store and applied (incremental engine).
    pub fn view(&self) -> ApiResult<Arc<View>> {
        if let Some(v) = lock(&self.cache)?.as_ref() {
            return Ok(v.clone());
        }
        // Read before the store: a change committed while this view is
        // computed bumps it, and the (stale) view is then not cached.
        let generation = self.generation.load(Ordering::SeqCst);
        let mut slot = lock(&self.engine)?;
        let after = slot.as_ref().map_or(0, |(_, row)| *row);
        let (new, last) = lock(&self.state)?
            .store
            .store()
            .ok_or_else(locked_error)?
            .events_since(after)
            .map_err(ApiError::internal)?;
        let engine = match slot.take() {
            Some((engine, _)) => {
                lock(&engine)?
                    .extend(&new, &self.keys)
                    .map_err(ApiError::internal)?;
                engine
            }
            None => {
                if new.is_empty() {
                    return Err(ApiError::new("not_found", "no organization on this device"));
                }
                Arc::new(Mutex::new(
                    Engine::build_for(new, &self.keys).map_err(ApiError::internal)?,
                ))
            }
        };
        *slot = Some((engine.clone(), last));
        drop(slot);
        let v = {
            let e = lock(&engine)?;
            Arc::new(View {
                events: e.events().to_vec(),
                proj: e.projection().clone(),
                ring: e.keyring().clone(),
                engine: engine.clone(),
            })
        };
        // Only the organization's devices may sync with this one.
        let allowed = v
            .proj
            .auth
            .devices
            .iter()
            .filter(|(_, d)| !d.revoked)
            .map(|(k, _)| *k)
            .collect();
        lock(&self.state)?.allowed = Some(allowed);
        let mut cache = lock(&self.cache)?;
        if self.generation.load(Ordering::SeqCst) == generation {
            *cache = Some(v.clone());
        }
        Ok(v)
    }

    pub fn me(&self) -> ApiResult<PersonId> {
        let v = self.view()?;
        v.proj
            .auth
            .author(&self.keys.public())
            .map_err(|_| ApiError::new("denied", "this device is not part of the organization yet"))
    }

    /// Stores locally created events, queues them and announces them.
    pub fn commit(self: &Arc<Self>, events: Vec<SignedEvent>) -> ApiResult<()> {
        {
            let mut st = lock(&self.state)?;
            let store = st.store.store_mut().ok_or_else(locked_error)?;
            for e in &events {
                store.insert(e, self.now()).map_err(ApiError::internal)?;
                store
                    .outbox_add(&e.id, self.now())
                    .map_err(ApiError::internal)?;
            }
        }
        self.invalidate();
        // Announce and sync once for a burst of local changes.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            if !self.sync_scheduled.swap(true, Ordering::SeqCst) {
                let core = self.clone();
                handle.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    core.sync_scheduled.store(false, Ordering::SeqCst);
                    if let Some(node) = core.node().await {
                        let heads = core
                            .view()
                            .map(|v| v.proj.heads.clone())
                            .unwrap_or_default();
                        let _ = node.announce(heads).await;
                        let _ = core.sync_round().await;
                    }
                    push::schedule_wake(core);
                });
            }
        }
        Ok(())
    }

    /// Runs an action as this device and commits its events.
    pub fn act<T>(
        self: &Arc<Self>,
        f: impl FnOnce(&Client<'_>) -> Result<(T, Vec<SignedEvent>), ActionError>,
    ) -> ApiResult<T> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let client = Client {
            events: &v.events,
            proj: &v.proj,
            keys: &self.keys,
            keyring: &v.ring,
            now: self.now(),
            engine: Some(v.engine.as_ref()),
        };
        let (value, events) = f(&client)?;
        self.commit(events)?;
        Ok(value)
    }

    pub fn act_events(
        self: &Arc<Self>,
        f: impl FnOnce(&Client<'_>) -> Result<Vec<SignedEvent>, ActionError>,
    ) -> ApiResult<()> {
        self.act(|c| f(c).map(|e| ((), e)))
    }

    // ----- network ------------------------------------------------------------

    /// Starts the endpoint (if a network is configured).
    /// Network parameters in the genesis of this device's organization.
    pub fn genesis_network(&self) -> Option<core_events::payloads::NetworkParams> {
        let st = self.state.lock().ok()?;
        let store = st.store.store()?;
        let org = store.org()?;
        let genesis = store.get(&core_events::EventHash(org.0)).ok()??;
        genesis
            .public_content::<core_events::payloads::GenesisPayload>()
            .ok()
            .map(|g| g.network)
    }

    /// The network to use: the server named in the genesis, else the
    /// configured one (`Local` always wins: tests).
    pub fn effective_network(&self) -> Option<Network> {
        let base = self.network.clone()?;
        if matches!(base, Network::Local) {
            return Some(base);
        }
        let params = self.genesis_network().or_else(|| self.server.clone());
        Some(match params {
            Some(p) if !p.relay_urls.is_empty() => Network::Server {
                relays: p.relay_urls,
                dns: p.dns_url,
            },
            _ => base,
        })
    }

    pub async fn start_network(self: &Arc<Self>) -> ApiResult<()> {
        let Some(network) = self.effective_network() else {
            return Ok(());
        };
        let mut guard = self.node.lock().await;
        if guard.is_some() {
            return Ok(());
        }
        let confirm = Arc::new(UiConfirm {
            pairings: self.pairings.clone(),
        });
        let node = Arc::new(
            Node::spawn(self.keys.clone(), self.state.clone(), network, confirm)
                .await
                .map_err(|e| ApiError::new("network", e.to_string()))?,
        );
        if self.has_org() {
            let peers = self
                .view()
                .map(|v| {
                    v.proj
                        .auth
                        .devices
                        .keys()
                        .copied()
                        .filter(|d| *d != self.keys.public())
                        .collect()
                })
                .unwrap_or_default();
            let _ = node.start_gossip(peers).await;
        }
        *guard = Some(node);
        Ok(())
    }

    pub async fn node(&self) -> Option<Arc<Node>> {
        self.node.lock().await.clone()
    }

    /// One round of synchronization with the peers chosen by the schedule.
    pub async fn sync_round(self: &Arc<Self>) -> ApiResult<u32> {
        let Some(node) = self.node().await else {
            return Ok(0);
        };
        if self.is_locked() {
            // Receive only, from the peers of the last unlocked round.
            let peers = lock(&self.peers_cache)?.clone();
            let mut new = 0;
            for p in peers {
                new += node
                    .sync_with(&p)
                    .await
                    .map(|s| s.events_new as u32)
                    .unwrap_or(0);
            }
            return Ok(new);
        }
        let Ok(v) = self.view() else {
            return Ok(0);
        };
        let pending: Vec<SignedEvent> = {
            let st = lock(&self.state)?;
            st.store
                .store()
                .map(|store| {
                    store
                        .outbox()
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|id| store.get(id).ok().flatten())
                        .collect()
                })
                .unwrap_or_default()
        };
        let targets = core_sync::schedule::outbox_targets(&v.proj, &pending);
        let peers = core_sync::schedule::peers(
            &v.proj,
            &self.keys.public(),
            &targets,
            false,
            (self.now() / 60_000) as u64,
        );
        *lock(&self.peers_cache)? = peers.clone();
        let mut tasks = tokio::task::JoinSet::new();
        for p in peers {
            let node = node.clone();
            tasks.spawn(async move {
                node.sync_with(&p)
                    .await
                    .map(|s| s.events_new as u32)
                    .unwrap_or(0)
            });
        }
        let mut new = 0;
        while let Some(r) = tasks.join_next().await {
            new += r.unwrap_or(0);
        }
        self.invalidate();
        if let Ok(v) = self.view() {
            let mut st = lock(&self.state)?;
            if let Some(store) = st.store.store_mut() {
                let _ = core_sync::delivery::update_outbox(store, &v.proj);
            }
        }
        let _ = self.publish_membership().await;
        let now = self.now();
        self.update_local(|l| l.last_sync = Some(now))?;
        Ok(new)
    }

    /// Joins through a ticket: binds the store, pairs and syncs.
    pub async fn join(self: &Arc<Self>, ticket: &str) -> ApiResult<String> {
        let t = core_sync::pairing::Ticket::decode(ticket).map_err(ApiError::invalid)?;
        self.start_network().await?;
        let node = self
            .node()
            .await
            .ok_or_else(|| ApiError::new("network", "no network configured"))?;
        let code =
            core_sync::pairing::verification_code(&t.secret, &t.inviter, &self.keys.identity());
        let core = self.clone();
        let inviter = t.inviter;
        tokio::spawn(async move {
            if let Ok(core_sync::pairing::PairResponse::Accepted { .. }) = node.pair(&t).await {
                // The inviter adds the device and distributes keys: retry until they arrive.
                for _ in 0..60 {
                    let _ = node.sync_with(&inviter).await;
                    core.invalidate();
                    if core.me().is_ok() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                let peers = core
                    .view()
                    .map(|v| v.proj.auth.devices.keys().copied().collect())
                    .unwrap_or_default();
                let _ = node.start_gossip(peers).await;
            }
        });
        Ok(code)
    }

    pub(crate) fn take_pairing_decision(
        &self,
        code: &str,
        accept: bool,
    ) -> ApiResult<Option<(PersonId, DeviceIdentity)>> {
        let mut p = lock(&self.pairings)?;
        let Some(mut pending) = p.pending.remove(code) else {
            return Err(ApiError::not_found("pairing request"));
        };
        if let Some(tx) = pending.decide.take() {
            let _ = tx.send(accept);
        }
        Ok(accept.then_some((pending.person, pending.identity)))
    }

    pub(crate) fn pending_pairings(&self) -> ApiResult<Vec<(PersonId, String)>> {
        Ok(lock(&self.pairings)?
            .pending
            .values()
            .map(|p| (p.person, p.code.clone()))
            .collect())
    }

    pub(crate) fn invitations(&self) -> &Shared {
        &self.state
    }
}

pub fn parse_id(s: &str) -> ApiResult<Id> {
    s.parse()
        .map_err(|_| ApiError::invalid(format!("invalid id `{s}`")))
}

pub fn parse_person(s: &str) -> ApiResult<PersonId> {
    parse_id(s).map(PersonId)
}
