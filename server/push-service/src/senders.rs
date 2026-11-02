//! Delivery to FCM (HTTP v1) and APNs (token auth). The push carries no
//! data: Android gets a high-priority data message `{ "t": "sync" }`, iOS a
//! visible notification with the organization's generic text.

use std::sync::Mutex;

use async_trait::async_trait;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};

use crate::db::Target;

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    /// The token is no longer valid: forget it.
    #[error("token no longer valid")]
    TokenGone,
    #[error("platform not configured: {0}")]
    NotConfigured(String),
    #[error("{0}")]
    Failed(String),
}

#[async_trait]
pub trait Sender: Send + Sync {
    async fn send(&self, target: &Target, notice: &str) -> Result<(), SendError>;
}

fn now_s() -> i64 {
    core_push::now_ms() / 1000
}

fn failed(e: impl std::fmt::Display) -> SendError {
    SendError::Failed(e.to_string())
}

#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: String,
}

#[derive(Serialize)]
struct GoogleClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct GoogleToken {
    access_token: String,
    expires_in: i64,
}

/// Firebase Cloud Messaging, HTTP v1, with a service account.
pub struct Fcm {
    project_id: String,
    client_email: String,
    key: EncodingKey,
    token_url: String,
    api_base: String,
    http: reqwest::Client,
    cached: Mutex<Option<(String, i64)>>,
}

impl Fcm {
    pub fn from_service_account(json: &str, http: reqwest::Client) -> Result<Self, SendError> {
        let sa: ServiceAccount = serde_json::from_str(json).map_err(failed)?;
        Ok(Self {
            key: EncodingKey::from_rsa_pem(sa.private_key.as_bytes()).map_err(failed)?,
            project_id: sa.project_id,
            client_email: sa.client_email,
            token_url: "https://oauth2.googleapis.com/token".into(),
            api_base: "https://fcm.googleapis.com".into(),
            http,
            cached: Mutex::new(None),
        })
    }

    /// Other endpoints (tests).
    pub fn with_urls(mut self, token_url: &str, api_base: &str) -> Self {
        self.token_url = token_url.into();
        self.api_base = api_base.into();
        self
    }

    async fn access_token(&self) -> Result<String, SendError> {
        let now = now_s();
        if let Some((t, exp)) = self.cached.lock().ok().and_then(|c| c.clone()) {
            if exp > now + 60 {
                return Ok(t);
            }
        }
        let claims = GoogleClaims {
            iss: &self.client_email,
            scope: "https://www.googleapis.com/auth/firebase.messaging",
            aud: &self.token_url,
            iat: now,
            exp: now + 3600,
        };
        let jwt = encode(&Header::new(Algorithm::RS256), &claims, &self.key).map_err(failed)?;
        let body = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={jwt}"
        );
        let res = self
            .http
            .post(&self.token_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(failed)?;
        if !res.status().is_success() {
            return Err(SendError::Failed(format!("FCM token: {}", res.status())));
        }
        let t: GoogleToken = res.json().await.map_err(failed)?;
        if let Ok(mut c) = self.cached.lock() {
            *c = Some((t.access_token.clone(), now + t.expires_in));
        }
        Ok(t.access_token)
    }

    pub async fn send(&self, token: &str) -> Result<(), SendError> {
        let access = self.access_token().await?;
        let body = serde_json::json!({
            "message": { "token": token, "data": { "t": "sync" }, "android": { "priority": "high" } }
        });
        let res = self
            .http
            .post(format!(
                "{}/v1/projects/{}/messages:send",
                self.api_base, self.project_id
            ))
            .bearer_auth(access)
            .json(&body)
            .send()
            .await
            .map_err(failed)?;
        match res.status().as_u16() {
            200..=299 => Ok(()),
            404 => Err(SendError::TokenGone),
            s => Err(SendError::Failed(format!("FCM: {s}"))),
        }
    }
}

#[derive(Serialize)]
struct ApnsClaims<'a> {
    iss: &'a str,
    iat: i64,
}

/// Apple Push Notification service, token-based (`.p8` key).
pub struct Apns {
    key: EncodingKey,
    key_id: String,
    team_id: String,
    base: String,
    http: reqwest::Client,
    cached: Mutex<Option<(String, i64)>>,
}

impl Apns {
    pub fn new(
        p8: &str,
        key_id: &str,
        team_id: &str,
        sandbox: bool,
        http: reqwest::Client,
    ) -> Result<Self, SendError> {
        Ok(Self {
            key: EncodingKey::from_ec_pem(p8.as_bytes()).map_err(failed)?,
            key_id: key_id.into(),
            team_id: team_id.into(),
            base: if sandbox {
                "https://api.sandbox.push.apple.com".into()
            } else {
                "https://api.push.apple.com".into()
            },
            http,
            cached: Mutex::new(None),
        })
    }

    pub fn with_base(mut self, base: &str) -> Self {
        self.base = base.into();
        self
    }

    /// Provider token, renewed every 50 minutes (Apple accepts up to 60).
    fn provider_token(&self) -> Result<String, SendError> {
        let now = now_s();
        if let Some((t, iat)) = self.cached.lock().ok().and_then(|c| c.clone()) {
            if now - iat < 50 * 60 {
                return Ok(t);
            }
        }
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        let t = encode(
            &header,
            &ApnsClaims {
                iss: &self.team_id,
                iat: now,
            },
            &self.key,
        )
        .map_err(failed)?;
        if let Ok(mut c) = self.cached.lock() {
            *c = Some((t.clone(), now));
        }
        Ok(t)
    }

    pub async fn send(&self, token: &str, topic: &str, notice: &str) -> Result<(), SendError> {
        let body = serde_json::json!({
            "aps": { "alert": { "body": notice }, "content-available": 1 },
            "t": "sync"
        });
        let res = self
            .http
            .post(format!("{}/3/device/{token}", self.base))
            .bearer_auth(self.provider_token()?)
            .header("apns-topic", topic)
            .header("apns-push-type", "alert")
            .header("apns-priority", "10")
            .json(&body)
            .send()
            .await
            .map_err(failed)?;
        match res.status().as_u16() {
            200..=299 => Ok(()),
            410 => Err(SendError::TokenGone),
            s => Err(SendError::Failed(format!("APNs: {s}"))),
        }
    }
}

/// Picks the platform of each target.
pub struct Dispatch {
    pub fcm: Option<Fcm>,
    pub apns: Option<Apns>,
}

#[async_trait]
impl Sender for Dispatch {
    async fn send(&self, t: &Target, notice: &str) -> Result<(), SendError> {
        match t.platform.as_str() {
            "fcm" => match &self.fcm {
                Some(f) => f.send(&t.token).await,
                None => Err(SendError::NotConfigured("fcm".into())),
            },
            "apns" => match &self.apns {
                Some(a) => a.send(&t.token, &t.app_id, notice).await,
                None => Err(SendError::NotConfigured("apns".into())),
            },
            other => Err(SendError::NotConfigured(other.into())),
        }
    }
}
