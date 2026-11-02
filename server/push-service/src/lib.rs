//! Push wake-up service (PROGETTO.md §10.1). It stores no events and no
//! content: only push tokens and, for each organization, which devices may
//! use it. Every request is signed by a device (see `core-push`).

pub mod db;
pub mod limits;
pub mod senders;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use core_push::{Membership, Platform, Register, Wake, WakeResult};
use serde::de::DeserializeOwned;

use crate::db::{Db, Org};
use crate::limits::{Decision, Limits};
use crate::senders::{SendError, Sender};

/// Most targets accepted in one wake request.
pub const MAX_TARGETS: usize = 500;
/// Most devices in one organization.
pub const MAX_DEVICES: usize = 10_000;

pub struct AppState {
    pub db: Mutex<Db>,
    pub limits: Mutex<Limits>,
    pub sender: Arc<dyn Sender>,
    /// iOS notice when the organization sets none.
    pub default_notice: String,
    pub clock: fn() -> i64,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/v1/register", post(register).delete(unregister))
        .route("/v1/org/membership", post(membership))
        .route("/v1/wake", post(wake))
        .with_state(state)
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, msg.to_owned()).into_response()
}

fn internal(e: impl std::fmt::Display) -> Response {
    // No content in the logs: only the kind of failure.
    eprintln!("internal error: {e}");
    err(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

/// A refused request: status and a short reason.
type Refusal = (StatusCode, &'static str);

/// Checks the signature headers; returns the device (hex).
fn authenticate(
    state: &AppState,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<String, Refusal> {
    let h = |n: &str| {
        headers
            .get(n)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
    };
    core_push::verify(
        method.as_str(),
        uri.path(),
        body,
        h("x-device"),
        h("x-timestamp"),
        h("x-signature"),
        (state.clock)(),
    )
    .map(|d| hex::encode(d.0))
    .map_err(|m| (StatusCode::UNAUTHORIZED, m))
}

fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, Refusal> {
    serde_json::from_slice(body).map_err(|_| (StatusCode::BAD_REQUEST, "malformed body"))
}

fn is_key(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

async fn register(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let device = match authenticate(&state, &method, &uri, &headers, &body) {
        Ok(d) => d,
        Err(r) => return r.into_response(),
    };
    let r: Register = match parse(&body) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    if r.token.is_empty() || r.token.len() > 4096 || r.app_id.is_empty() || r.app_id.len() > 255 {
        return err(StatusCode::BAD_REQUEST, "bad token or app id");
    }
    let Ok(db) = state.db.lock() else {
        return internal("lock");
    };
    match db.org(&r.org) {
        Ok(Some(o)) if o.devices.contains(&device) => {}
        Ok(_) => return err(StatusCode::FORBIDDEN, "device not in the organization"),
        Err(e) => return internal(e),
    }
    let platform = match r.platform {
        Platform::Fcm => "fcm",
        Platform::Apns => "apns",
    };
    match db.register(
        &device,
        &r.org,
        platform,
        &r.token,
        &r.app_id,
        (state.clock)(),
    ) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}

async fn unregister(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let device = match authenticate(&state, &method, &uri, &headers, &body) {
        Ok(d) => d,
        Err(r) => return r.into_response(),
    };
    let Ok(db) = state.db.lock() else {
        return internal("lock");
    };
    match db.unregister(&device) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}

/// Root administrators' devices of a genesis, if it is the genesis of `org`.
fn genesis_admins(org: &str, genesis_hex: &str) -> Option<Vec<String>> {
    let bytes = hex::decode(genesis_hex).ok()?;
    let event = core_events::SignedEvent::from_bytes(&bytes).ok()?;
    let info = core_events::genesis::check_genesis(&event).ok()?;
    if hex::encode(event.org_id().0) != org {
        return None;
    }
    Some(
        info.payload
            .root_admins
            .iter()
            .flat_map(|a| a.devices.iter().map(|d| hex::encode(d.signing.0)))
            .collect(),
    )
}

async fn membership(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let device = match authenticate(&state, &method, &uri, &headers, &body) {
        Ok(d) => d,
        Err(r) => return r.into_response(),
    };
    let m: Membership = match parse(&body) {
        Ok(m) => m,
        Err(e) => return e.into_response(),
    };
    let lists_ok = m.devices.len() <= MAX_DEVICES
        && m.devices.iter().all(|d| is_key(d))
        && m.admins
            .as_ref()
            .is_none_or(|a| a.len() <= MAX_DEVICES && a.iter().all(|d| is_key(d)));
    if !lists_ok || !is_key(&m.org) || m.notice_text.as_ref().is_some_and(|t| t.len() > 200) {
        return err(StatusCode::BAD_REQUEST, "bad lists");
    }
    let Ok(db) = state.db.lock() else {
        return internal("lock");
    };
    let existing = match db.org(&m.org) {
        Ok(o) => o,
        Err(e) => return internal(e),
    };
    let admins: Vec<String> = match existing {
        // First call: the caller proves it is a root administrator.
        None => {
            let Some(roots) = m.genesis.as_deref().and_then(|g| genesis_admins(&m.org, g)) else {
                return err(
                    StatusCode::FORBIDDEN,
                    "first registration needs the genesis",
                );
            };
            if !roots.contains(&device) {
                return err(StatusCode::FORBIDDEN, "not a root administrator");
            }
            let mut all: BTreeSet<String> = roots.into_iter().collect();
            all.extend(m.admins.clone().unwrap_or_default());
            all.into_iter().collect()
        }
        Some(o) => {
            if !o.admins.contains(&device) {
                return err(StatusCode::FORBIDDEN, "not an administrator");
            }
            m.admins.clone().unwrap_or(o.admins)
        }
    };
    // Administrators are always allowed devices.
    let devices: BTreeSet<String> = m
        .devices
        .iter()
        .cloned()
        .chain(admins.iter().cloned())
        .collect();
    let org = Org {
        devices: devices.into_iter().collect(),
        admins,
        notice_text: m.notice_text.clone(),
    };
    match db.put_org(&m.org, &org, &m.proof, (state.clock)()) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => internal(e),
    }
}

async fn wake(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let device = match authenticate(&state, &method, &uri, &headers, &body) {
        Ok(d) => d,
        Err(r) => return r.into_response(),
    };
    let w: Wake = match parse(&body) {
        Ok(w) => w,
        Err(e) => return e.into_response(),
    };
    if w.targets.len() > MAX_TARGETS {
        return err(StatusCode::BAD_REQUEST, "too many targets");
    }
    let now = (state.clock)();
    let (org, targets) = {
        let Ok(db) = state.db.lock() else {
            return internal("lock");
        };
        let org = match db.org(&w.org) {
            Ok(Some(o)) if o.devices.contains(&device) => o,
            Ok(_) => return err(StatusCode::FORBIDDEN, "device not in the organization"),
            Err(e) => return internal(e),
        };
        let unique: BTreeSet<&String> = w.targets.iter().collect();
        let targets: Vec<(String, Option<db::Target>)> = unique
            .into_iter()
            .map(|t| {
                let target = org
                    .devices
                    .contains(t)
                    .then(|| db.target(t, &w.org).ok().flatten())
                    .flatten();
                (t.clone(), target)
            })
            .collect();
        (org, targets)
    };
    {
        let Ok(mut limits) = state.limits.lock() else {
            return internal("lock");
        };
        if !limits.allow_sender(&device, now) {
            return err(StatusCode::TOO_MANY_REQUESTS, "too many wake requests");
        }
    }
    let notice = org
        .notice_text
        .clone()
        .unwrap_or_else(|| state.default_notice.clone());
    let mut result = WakeResult::default();
    for (key, target) in targets {
        let Some(target) = target else {
            result.unknown += 1;
            continue;
        };
        let decision = match state.limits.lock() {
            Ok(mut l) => l.recipient(&key, now),
            Err(_) => return internal("lock"),
        };
        match decision {
            Decision::Coalesced => result.coalesced += 1,
            Decision::Limited => result.limited += 1,
            Decision::Send => match state.sender.send(&target, &notice).await {
                Ok(()) => result.sent += 1,
                Err(SendError::TokenGone) => {
                    if let Ok(db) = state.db.lock() {
                        let _ = db.unregister(&key);
                    }
                    result.unknown += 1;
                }
                Err(e) => {
                    eprintln!("push failed: {e}");
                    result.unknown += 1;
                }
            },
        }
    }
    Json(result).into_response()
}
