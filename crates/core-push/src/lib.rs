//! Client of the push wake-up service (PROGETTO.md §10.1, §11).
//!
//! Every request is signed by the device: `X-Device` (public key, hex),
//! `X-Timestamp` (ms) and `X-Signature` = Ed25519 over
//! `method || path || timestamp || blake3(body)`. The server uses
//! [`signing_message`] and [`verify`] from this crate, so both sides agree.
//!
//! No content ever goes through the service: only device keys, push tokens
//! and "wake up" requests.

use std::sync::Arc;

use core_crypto::{DeviceKeys, DevicePublicKey, Signature};
use serde::{Deserialize, Serialize};

/// Accepted clock difference between device and server.
pub const MAX_SKEW_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("network: {0}")]
    Network(String),
    #[error("refused by the push service ({status}): {message}")]
    Refused { status: u16, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Fcm,
    Apns,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Register {
    /// Organization id (hex).
    pub org: String,
    pub platform: Platform,
    pub token: String,
    /// Android package or iOS bundle id.
    pub app_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    pub org: String,
    /// Devices allowed to register and wake (hex public keys).
    pub devices: Vec<String>,
    /// Devices allowed to change this list; `None` = unchanged.
    #[serde(default)]
    pub admins: Option<Vec<String>>,
    /// Ids of the events that justify the list (kept for audit, not verified).
    #[serde(default)]
    pub proof: Vec<String>,
    /// First call for an organization: its genesis event (bytes, hex), so the
    /// service can check the caller is one of its root administrators.
    #[serde(default)]
    pub genesis: Option<String>,
    /// Text of the visible iOS notification (generic, no data).
    #[serde(default)]
    pub notice_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wake {
    pub org: String,
    pub targets: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeResult {
    /// Pushes sent.
    pub sent: u32,
    /// Merged with a push sent less than 2 minutes before.
    pub coalesced: u32,
    /// Over the hourly limits.
    pub limited: u32,
    /// Not in the organization, or without a token.
    pub unknown: u32,
}

/// What is signed for a request.
pub fn signing_message(method: &str, path: &str, timestamp: i64, body: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(method.len() + path.len() + 8 + 32);
    m.extend_from_slice(method.as_bytes());
    m.extend_from_slice(path.as_bytes());
    m.extend_from_slice(&timestamp.to_le_bytes());
    m.extend_from_slice(blake3::hash(body).as_bytes());
    m
}

/// Checks the three headers; returns the signing device.
pub fn verify(
    method: &str,
    path: &str,
    body: &[u8],
    device: &str,
    timestamp: &str,
    signature: &str,
    now_ms: i64,
) -> Result<DevicePublicKey, &'static str> {
    let key: [u8; 32] = hex::decode(device)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or("bad X-Device")?;
    let ts: i64 = timestamp.parse().map_err(|_| "bad X-Timestamp")?;
    if (now_ms - ts).abs() > MAX_SKEW_MS {
        return Err("X-Timestamp out of range");
    }
    let sig: [u8; 64] = hex::decode(signature)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or("bad X-Signature")?;
    let device = DevicePublicKey(key);
    device
        .verify(&signing_message(method, path, ts, body), &Signature(sig))
        .map_err(|_| "signature does not match")?;
    Ok(device)
}

/// Installs the TLS crypto provider (ring) once per process.
pub fn ensure_tls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Signed client of one push service.
#[derive(Clone)]
pub struct PushClient {
    base: String,
    keys: Arc<DeviceKeys>,
    http: reqwest::Client,
}

impl PushClient {
    /// `base` like `https://api.example.org` (no trailing slash needed).
    pub fn new(base: &str, keys: Arc<DeviceKeys>) -> Result<Self, PushError> {
        ensure_tls_provider();
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| PushError::Network(e.to_string()))?;
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            keys,
            http,
        })
    }

    async fn call<T: Serialize, R: for<'de> Deserialize<'de> + Default>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&T>,
    ) -> Result<R, PushError> {
        let bytes = match body {
            Some(b) => serde_json::to_vec(b).map_err(|e| PushError::Network(e.to_string()))?,
            None => Vec::new(),
        };
        let ts = now_ms();
        let sig = self
            .keys
            .sign(&signing_message(method.as_str(), path, ts, &bytes));
        let res = self
            .http
            .request(method, format!("{}{path}", self.base))
            .header("content-type", "application/json")
            .header("X-Device", hex::encode(self.keys.public().0))
            .header("X-Timestamp", ts.to_string())
            .header("X-Signature", hex::encode(sig.0))
            .body(bytes)
            .send()
            .await
            .map_err(|e| PushError::Network(e.to_string()))?;
        let status = res.status();
        let text = res
            .text()
            .await
            .map_err(|e| PushError::Network(e.to_string()))?;
        if !status.is_success() {
            return Err(PushError::Refused {
                status: status.as_u16(),
                message: text,
            });
        }
        if text.trim().is_empty() {
            return Ok(R::default());
        }
        serde_json::from_str(&text).map_err(|e| PushError::Network(e.to_string()))
    }

    pub async fn register(&self, r: &Register) -> Result<(), PushError> {
        self.call::<_, serde_json::Value>(reqwest::Method::POST, "/v1/register", Some(r))
            .await
            .map(|_| ())
    }

    pub async fn unregister(&self) -> Result<(), PushError> {
        self.call::<(), serde_json::Value>(reqwest::Method::DELETE, "/v1/register", None)
            .await
            .map(|_| ())
    }

    pub async fn membership(&self, m: &Membership) -> Result<(), PushError> {
        self.call::<_, serde_json::Value>(reqwest::Method::POST, "/v1/org/membership", Some(m))
            .await
            .map(|_| ())
    }

    pub async fn wake(&self, w: &Wake) -> Result<WakeResult, PushError> {
        self.call(reqwest::Method::POST, "/v1/wake", Some(w)).await
    }
}

/// Push support of a platform (§11.3). Desktop: [`NoPush`].
pub trait PushProvider: Send + Sync {
    /// Current push token, if the platform has one.
    fn token(&self) -> Option<(Platform, String)>;
}

/// Desktop: no push (the app stays connected from the tray).
pub struct NoPush;

impl PushProvider for NoPush {
    fn token(&self) -> Option<(Platform, String)> {
        None
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn signature_round_trip() {
        let keys = DeviceKeys::generate().unwrap();
        let body = br#"{"a":1}"#;
        let ts = 1_000_000;
        let sig = keys.sign(&signing_message("POST", "/v1/wake", ts, body));
        let dev = hex::encode(keys.public().0);
        let s = hex::encode(sig.0);
        assert_eq!(
            verify(
                "POST",
                "/v1/wake",
                body,
                &dev,
                &ts.to_string(),
                &s,
                ts + 1000
            )
            .unwrap(),
            keys.public()
        );
        // Another body, path or method, or a stale timestamp: refused.
        assert!(verify("POST", "/v1/wake", b"{}", &dev, &ts.to_string(), &s, ts).is_err());
        assert!(verify("POST", "/v1/register", body, &dev, &ts.to_string(), &s, ts).is_err());
        assert!(verify("DELETE", "/v1/wake", body, &dev, &ts.to_string(), &s, ts).is_err());
        assert!(
            verify(
                "POST",
                "/v1/wake",
                body,
                &dev,
                &ts.to_string(),
                &s,
                ts + MAX_SKEW_MS + 1
            )
            .is_err()
        );
    }
}
