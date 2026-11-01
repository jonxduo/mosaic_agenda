//! Facade for the apps (PROGETTO.md §12): one [`Core`] per device, with
//! typed commands returning DTOs whose labels are already resolved.
//!
//! The desktop and mobile apps expose these commands through Tauri; the
//! development HTTP bridge (`api-dev-server`) exposes the same ones to the
//! browser and to the Playwright tests.

pub mod commands;
pub mod dispatch;
pub mod dto;
pub mod secrets;
pub mod texts;
pub mod views;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use core_config::CompiledConfig;
use core_crypto::{ChannelKeyring, DeviceIdentity, DeviceKeys, SecretStore};
use core_engine::actions::{ActionError, Client};
use core_engine::channels::derive_keyring;
use core_engine::{Projection, rebuild};
use core_events::{DbKey, EventStore, SignedEvent};
use core_model::{Id, PersonId};
use core_sync::blobs::DirBlobStore;
use core_sync::net::{ConfirmFuture, Network, Node, NodeState, PairConfirm, Shared};
use core_sync::pairing::Invitations;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::oneshot;

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
}

/// A decoded view of the local events, rebuilt when they change.
pub struct View {
    pub events: Vec<SignedEvent>,
    pub proj: Projection,
    pub ring: ChannelKeyring,
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
    pairings: Arc<Mutex<Pairings>>,
    local: Mutex<LocalSettings>,
    locked: AtomicBool,
    sync_scheduled: AtomicBool,
    clock: Option<i64>,
}

fn lock<T>(m: &Mutex<T>) -> ApiResult<std::sync::MutexGuard<'_, T>> {
    m.lock().map_err(|_| ApiError::internal("state poisoned"))
}

const PIN_SECRET: &str = "app.pin";

impl Core {
    pub fn open(opts: CoreOptions) -> ApiResult<Arc<Self>> {
        std::fs::create_dir_all(&opts.data_dir).map_err(ApiError::internal)?;
        let package = core_config::decode(&opts.bundle)
            .map_err(|e| ApiError::internal(format!("package: {e}")))?;
        let keys = Arc::new(
            DeviceKeys::load_or_create(opts.secrets.as_ref()).map_err(ApiError::internal)?,
        );
        let db_key = DbKey::load_or_create(opts.secrets.as_ref()).map_err(ApiError::internal)?;
        let store = EventStore::open(&opts.data_dir.join("events.db"), &db_key)
            .map_err(ApiError::internal)?;
        let blobs = DirBlobStore::new(opts.data_dir.join("blobs")).map_err(ApiError::internal)?;
        let local: LocalSettings = std::fs::read(opts.data_dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let has_pin = opts.secrets.get(PIN_SECRET).ok().flatten().is_some();
        let state = Arc::new(Mutex::new(NodeState {
            store,
            blobs: Box::new(blobs),
            wanted_blobs: Default::default(),
            allowed: None,
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
            pairings: Arc::new(Mutex::new(Pairings::default())),
            local: Mutex::new(local),
            locked: AtomicBool::new(has_pin),
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

    fn pin_hash(&self, pin: &str) -> [u8; 32] {
        let mut h = blake3::Hasher::new_derive_key("org-app pin v1");
        h.update(&self.keys.public().0);
        h.update(pin.as_bytes());
        *h.finalize().as_bytes()
    }

    pub fn set_pin(&self, pin: Option<String>) -> ApiResult<()> {
        match pin {
            Some(p) if p.chars().count() >= 4 => self
                .secrets
                .put(PIN_SECRET, &self.pin_hash(&p))
                .map_err(ApiError::internal),
            Some(_) => Err(ApiError::invalid("the PIN needs at least 4 digits")),
            None => self.secrets.delete(PIN_SECRET).map_err(ApiError::internal),
        }
    }

    pub fn unlock(&self, pin: &str) -> ApiResult<bool> {
        let stored = self.secrets.get(PIN_SECRET).map_err(ApiError::internal)?;
        let ok = match stored {
            None => true,
            Some(h) => h.as_slice() == self.pin_hash(pin),
        };
        if ok {
            self.locked.store(false, Ordering::SeqCst);
        }
        Ok(ok)
    }

    pub fn lock_now(&self) {
        let has_pin = self.secrets.get(PIN_SECRET).ok().flatten().is_some();
        if has_pin {
            self.locked.store(true, Ordering::SeqCst);
        }
    }

    pub(crate) fn has_pin(&self) -> bool {
        self.secrets.get(PIN_SECRET).ok().flatten().is_some()
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
        if let Ok(mut c) = self.cache.lock() {
            *c = None;
        }
    }

    /// Current view of the local events (cached).
    pub fn view(&self) -> ApiResult<Arc<View>> {
        if let Some(v) = lock(&self.cache)?.as_ref() {
            return Ok(v.clone());
        }
        let events = lock(&self.state)?
            .store
            .all_events()
            .map_err(ApiError::internal)?;
        if events.is_empty() {
            return Err(ApiError::new("not_found", "no organization on this device"));
        }
        let ring = derive_keyring(&events, &self.keys);
        let proj = rebuild(&events, &ring).map_err(ApiError::internal)?;
        // Only the organization's devices may sync with this one.
        let allowed = proj
            .auth
            .devices
            .iter()
            .filter(|(_, d)| !d.revoked)
            .map(|(k, _)| *k)
            .collect();
        lock(&self.state)?.allowed = Some(allowed);
        let v = Arc::new(View { events, proj, ring });
        *lock(&self.cache)? = Some(v.clone());
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
            for e in &events {
                st.store.insert(e, self.now()).map_err(ApiError::internal)?;
                st.store
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
    pub async fn start_network(self: &Arc<Self>) -> ApiResult<()> {
        let Some(network) = self.network.clone() else {
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
        let Ok(v) = self.view() else {
            return Ok(0);
        };
        let pending: Vec<SignedEvent> = {
            let st = lock(&self.state)?;
            st.store
                .outbox()
                .unwrap_or_default()
                .iter()
                .filter_map(|id| st.store.get(id).ok().flatten())
                .collect()
        };
        let targets = core_sync::schedule::outbox_targets(&v.proj, &pending);
        let peers = core_sync::schedule::peers(
            &v.proj,
            &self.keys.public(),
            &targets,
            false,
            (self.now() / 60_000) as u64,
        );
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
            let _ = core_sync::delivery::update_outbox(&mut st.store, &v.proj);
        }
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
