//! Wake-ups through the push service (§10.1, §11.2): membership published by
//! a root administrator, tokens registered by phones (F7), and "wake up"
//! requests for recipients that did not confirm receipt.

use std::collections::BTreeSet;
use std::sync::Arc;

use core_crypto::DevicePublicKey;
use core_push::{Membership, Platform, PushClient, PushError, Register, Wake};

use crate::{ApiError, ApiResult, Core, lock};

/// Wait for a direct delivery before asking for a wake-up.
pub const WAKE_AFTER_MS: u64 = 10_000;
/// At most one wake-up request per recipient device in this window.
pub const WAKE_COALESCE_MS: i64 = 2 * 60 * 1000;

fn net(e: PushError) -> ApiError {
    ApiError::new("network", e.to_string())
}

impl Core {
    /// Client of the push service named in the genesis (or the build).
    pub(crate) fn push_client(&self) -> Option<PushClient> {
        let url = self
            .genesis_network()
            .or_else(|| self.server.clone())?
            .push_url?;
        let mut cached = lock(&self.push).ok()?;
        if let Some((u, c)) = cached.as_ref() {
            if *u == url {
                return Some(c.clone());
            }
        }
        let client = PushClient::new(&url, self.keys.clone()).ok()?;
        *cached = Some((url, client.clone()));
        Some(client)
    }

    fn org_hex(&self) -> Option<String> {
        let st = self.state.lock().ok()?;
        st.store.org().map(|o| hex::encode(o.0))
    }

    /// Sends the list of the organization's devices to the push service when
    /// it changed. Only root administrators do it. Returns whether it sent.
    pub async fn publish_membership(&self) -> ApiResult<bool> {
        let Some(client) = self.push_client() else {
            return Ok(false);
        };
        let v = self.view()?;
        let me = self.me()?;
        if !v.proj.auth.is_root(me) {
            return Ok(false);
        }
        let Some(org) = self.org_hex() else {
            return Ok(false);
        };
        let active: Vec<(&DevicePublicKey, &core_engine::auth::DeviceRecord)> = v
            .proj
            .auth
            .devices
            .iter()
            .filter(|(_, d)| !d.revoked)
            .collect();
        let devices: Vec<String> = active.iter().map(|(k, _)| hex::encode(k.0)).collect();
        let admins: Vec<String> = active
            .iter()
            .filter(|(_, d)| v.proj.auth.is_root(d.person))
            .map(|(k, _)| hex::encode(k.0))
            .collect();
        let notice = self.texts().get("push.generic");
        let fingerprint = blake3::hash(format!("{org}|{devices:?}|{admins:?}|{notice}").as_bytes())
            .to_hex()
            .to_string();
        if self.local()?.push_published.as_deref() == Some(fingerprint.as_str()) {
            return Ok(false);
        }
        let mut m = Membership {
            org,
            devices,
            admins: Some(admins),
            proof: Vec::new(),
            genesis: None,
            notice_text: Some(notice),
        };
        match client.membership(&m).await {
            Ok(()) => {}
            // First time: prove we are a root administrator with the genesis.
            Err(PushError::Refused { status: 403, .. }) => {
                m.genesis = v
                    .events
                    .first()
                    .and_then(|g| g.to_bytes().ok())
                    .map(hex::encode);
                client.membership(&m).await.map_err(net)?;
            }
            Err(e) => return Err(net(e)),
        }
        self.update_local(|l| l.push_published = Some(fingerprint))?;
        Ok(true)
    }

    /// Registers this device's push token (phones; desktop has none).
    pub async fn register_push(
        &self,
        platform: Platform,
        token: String,
        app_id: String,
    ) -> ApiResult<()> {
        let client = self
            .push_client()
            .ok_or_else(|| ApiError::invalid("no push service configured"))?;
        let org = self
            .org_hex()
            .ok_or_else(|| ApiError::invalid("no organization on this device"))?;
        client
            .register(&Register {
                org,
                platform,
                token,
                app_id,
            })
            .await
            .map_err(net)
    }

    /// Asks the push service to wake the devices that should have received
    /// this device's pending events and did not confirm it (§11.2).
    /// Returns how many devices were asked for.
    pub async fn wake_missing(&self) -> ApiResult<u32> {
        let Some(client) = self.push_client() else {
            return Ok(0);
        };
        let Some(org) = self.org_hex() else {
            return Ok(0);
        };
        let v = self.view()?;
        let mine = self.keys.public();
        let mut targets: BTreeSet<DevicePublicKey> = BTreeSet::new();
        {
            let st = lock(&self.state)?;
            let Some(store) = st.store.store() else {
                return Ok(0);
            };
            for id in store.outbox().unwrap_or_default() {
                let Some(e) = store.get(&id).ok().flatten() else {
                    continue;
                };
                let got = store.receipts(&id).unwrap_or_default();
                for d in core_sync::schedule::outbox_targets(&v.proj, std::slice::from_ref(&e)) {
                    if d != mine && !got.contains(&d) {
                        targets.insert(d);
                    }
                }
            }
        }
        let now = self.now();
        let targets: Vec<String> = {
            let mut woken = lock(&self.woken)?;
            woken.retain(|_, t| now - *t < WAKE_COALESCE_MS);
            let fresh: Vec<DevicePublicKey> = targets
                .into_iter()
                .filter(|d| !woken.contains_key(d))
                .collect();
            for d in &fresh {
                woken.insert(*d, now);
            }
            fresh.iter().map(|d| hex::encode(d.0)).collect()
        };
        if targets.is_empty() {
            return Ok(0);
        }
        let n = targets.len() as u32;
        client
            .wake(&Wake {
                org,
                targets,
                reason: "sync".into(),
            })
            .await
            .map_err(net)?;
        Ok(n)
    }
}

/// After a burst of local events: wait for direct delivery, then wake.
pub(crate) fn schedule_wake(core: Arc<Core>) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(WAKE_AFTER_MS)).await;
            let _ = core.wake_missing().await;
        });
    }
}
