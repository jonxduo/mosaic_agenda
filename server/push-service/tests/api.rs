//! The push service end to end: the `core-push` client against a running
//! service, with a fake sender; then the FCM and APNs senders against fake
//! Google and Apple endpoints, with keys generated for the test.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use core_crypto::DeviceKeys;
use core_events::genesis::{NewOrganization, create_genesis};
use core_events::payloads::{NetworkParams, RootAdmin, SecurityParams};
use core_push::{Membership, Platform, PushClient, PushError, Register, Wake};
use push_service::db::{Db, Target};
use push_service::senders::{Apns, Fcm, SendError, Sender};
use push_service::{AppState, router};

#[derive(Default)]
struct FakeSender {
    sent: Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl Sender for FakeSender {
    async fn send(&self, t: &Target, notice: &str) -> Result<(), SendError> {
        if t.token == "gone" {
            return Err(SendError::TokenGone);
        }
        self.sent
            .lock()
            .unwrap()
            .push((t.token.clone(), notice.to_owned()));
        Ok(())
    }
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn hexkey(k: &DeviceKeys) -> String {
    hex::encode(k.public().0)
}

fn genesis(admin: &DeviceKeys) -> (String, String) {
    let org = NewOrganization {
        name: "Org".into(),
        root_admins: vec![RootAdmin {
            person: core_model::PersonId(core_model::Id::from_bytes([1; 16])),
            devices: vec![admin.identity()],
        }],
        config_bundle: vec![1, 2, 3],
        network: NetworkParams {
            relay_urls: vec![],
            dns_url: None,
            push_url: None,
        },
        security: SecurityParams::default(),
    };
    let (id, g) = create_genesis(org, admin, 0).unwrap();
    (hex::encode(id.0), hex::encode(g.to_bytes().unwrap()))
}

fn refused(r: Result<(), PushError>) -> u16 {
    match r {
        Err(PushError::Refused { status, .. }) => status,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn register_membership_and_wake() {
    core_push::ensure_tls_provider();
    let sender = Arc::new(FakeSender::default());
    let state = Arc::new(AppState {
        db: Mutex::new(Db::open_in_memory().unwrap()),
        limits: Mutex::new(Default::default()),
        sender: sender.clone(),
        default_notice: "Updates".into(),
        clock: core_push::now_ms,
    });
    let base = serve(router(state)).await;
    let admin = Arc::new(DeviceKeys::generate().unwrap());
    let phone = Arc::new(DeviceKeys::generate().unwrap());
    let other = Arc::new(DeviceKeys::generate().unwrap());
    let stranger = Arc::new(DeviceKeys::generate().unwrap());
    let (org, genesis_hex) = genesis(&admin);
    let a = PushClient::new(&base, admin.clone()).unwrap();
    let p = PushClient::new(&base, phone.clone()).unwrap();
    let o = PushClient::new(&base, other.clone()).unwrap();
    let s = PushClient::new(&base, stranger.clone()).unwrap();
    let reg = |token: &str| Register {
        org: org.clone(),
        platform: Platform::Fcm,
        token: token.into(),
        app_id: "org.example.app".into(),
    };
    let members = |genesis: Option<String>| Membership {
        org: org.clone(),
        devices: vec![hexkey(&phone), hexkey(&other)],
        admins: None,
        proof: vec!["e1".into()],
        genesis,
        notice_text: Some("There are updates".into()),
    };

    // Unknown organization: nobody can register; the first membership needs
    // the genesis and a root administrator.
    assert_eq!(refused(p.register(&reg("t-phone")).await), 403);
    assert_eq!(refused(a.membership(&members(None)).await), 403);
    assert_eq!(
        refused(s.membership(&members(Some(genesis_hex.clone()))).await),
        403
    );
    a.membership(&members(Some(genesis_hex))).await.unwrap();
    // Afterwards only administrators change the list.
    assert_eq!(refused(p.membership(&members(None)).await), 403);

    p.register(&reg("t-phone")).await.unwrap();
    o.register(&reg("gone")).await.unwrap();
    assert_eq!(refused(s.register(&reg("t-stranger")).await), 403);

    let wake = |targets: Vec<String>| Wake {
        org: org.clone(),
        targets,
        reason: "sync".into(),
    };
    let r = a
        .wake(&wake(vec![
            hexkey(&phone),
            hexkey(&other),
            hexkey(&stranger),
        ]))
        .await
        .unwrap();
    assert_eq!((r.sent, r.unknown), (1, 2), "{r:?}");
    assert_eq!(
        sender.sent.lock().unwrap().as_slice(),
        &[("t-phone".to_owned(), "There are updates".to_owned())]
    );
    // Within 2 minutes: merged.
    let r = o.wake(&wake(vec![hexkey(&phone)])).await.unwrap();
    assert_eq!(r.coalesced, 1);
    // The dead token was forgotten.
    let r = a.wake(&wake(vec![hexkey(&other)])).await.unwrap();
    assert_eq!(r.unknown, 1);
    // Strangers cannot wake anyone.
    assert!(matches!(
        s.wake(&wake(vec![hexkey(&phone)])).await,
        Err(PushError::Refused { status: 403, .. })
    ));

    // Removing a device from the organization drops its token.
    let mut m = members(None);
    m.devices = vec![hexkey(&other)];
    a.membership(&m).await.unwrap();
    assert_eq!(refused(p.register(&reg("t-phone")).await), 403);
    p.unregister().await.unwrap();
}

#[tokio::test]
async fn bad_signatures_are_refused() {
    let state = Arc::new(AppState {
        db: Mutex::new(Db::open_in_memory().unwrap()),
        limits: Mutex::new(Default::default()),
        sender: Arc::new(FakeSender::default()),
        default_notice: "x".into(),
        clock: core_push::now_ms,
    });
    let base = serve(router(state)).await;
    let http = reqwest::Client::new();
    let res = http
        .post(format!("{base}/v1/wake"))
        .header("X-Device", "00".repeat(32))
        .header("X-Timestamp", core_push::now_ms().to_string())
        .header("X-Signature", "00".repeat(64))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let res = http.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(res.text().await.unwrap(), "ok");
}

#[derive(Default)]
struct Seen {
    fcm: Vec<serde_json::Value>,
    apns: Vec<(String, String, serde_json::Value)>,
}

#[tokio::test]
async fn fcm_and_apns_requests() {
    use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
    use p256::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};

    core_push::ensure_tls_provider();
    let rsa_key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
    let rsa_pem = rsa_key.to_pkcs8_pem(LineEnding::LF).unwrap().to_string();
    let rsa_pub = rsa::RsaPublicKey::from(&rsa_key)
        .to_public_key_pem(LineEnding::LF)
        .unwrap();
    let ec_key = p256::SecretKey::random(&mut rand::rngs::OsRng);
    let ec_pem = ec_key.to_pkcs8_pem(LineEnding::LF).unwrap().to_string();
    let ec_pub = ec_key
        .public_key()
        .to_public_key_pem(LineEnding::LF)
        .unwrap();

    type S = Arc<(Mutex<Seen>, String, String)>;
    let shared: S = Arc::new((Mutex::new(Seen::default()), rsa_pub, ec_pub));
    let fake = Router::new()
        .route(
            "/token",
            post(|State(s): State<S>, body: String| async move {
                let jwt = body.split("assertion=").nth(1).unwrap().to_owned();
                let mut v = Validation::new(Algorithm::RS256);
                v.validate_aud = false;
                let claims = decode::<serde_json::Value>(
                    &jwt,
                    &DecodingKey::from_rsa_pem(s.1.as_bytes()).unwrap(),
                    &v,
                );
                assert!(claims.is_ok(), "service account JWT signed with the key");
                axum::Json(serde_json::json!({ "access_token": "abc", "expires_in": 3600 }))
            }),
        )
        .route(
            "/v1/projects/proj/messages:send",
            post(
                |State(s): State<S>,
                 headers: HeaderMap,
                 axum::Json(body): axum::Json<serde_json::Value>| async move {
                    assert_eq!(headers["authorization"], "Bearer abc");
                    let token = body["message"]["token"].as_str().unwrap().to_owned();
                    s.0.lock().unwrap().fcm.push(body);
                    if token == "gone" {
                        StatusCode::NOT_FOUND
                    } else {
                        StatusCode::OK
                    }
                },
            ),
        )
        .route(
            "/3/device/{token}",
            post(
                |State(s): State<S>,
                 Path(token): Path<String>,
                 headers: HeaderMap,
                 axum::Json(body): axum::Json<serde_json::Value>| async move {
                    let jwt = headers["authorization"]
                        .to_str()
                        .unwrap()
                        .trim_start_matches("Bearer ")
                        .to_owned();
                    let mut v = Validation::new(Algorithm::ES256);
                    v.required_spec_claims.clear();
                    v.validate_exp = false;
                    let ok = decode::<serde_json::Value>(
                        &jwt,
                        &DecodingKey::from_ec_pem(s.2.as_bytes()).unwrap(),
                        &v,
                    )
                    .unwrap();
                    assert_eq!(ok.header.kid.as_deref(), Some("KEYID"));
                    assert_eq!(ok.claims["iss"], "TEAM");
                    let topic = headers["apns-topic"].to_str().unwrap().to_owned();
                    s.0.lock().unwrap().apns.push((token.clone(), topic, body));
                    if token == "gone" {
                        StatusCode::GONE
                    } else {
                        StatusCode::OK
                    }
                },
            ),
        )
        .with_state(shared.clone());
    let base = serve(fake).await;
    let http = reqwest::Client::new();
    let sa = serde_json::json!({ "project_id": "proj", "client_email": "svc@example.org", "private_key": rsa_pem }).to_string();
    let fcm = Fcm::from_service_account(&sa, http.clone())
        .unwrap()
        .with_urls(&format!("{base}/token"), &base);
    fcm.send("t1").await.unwrap();
    fcm.send("t2").await.unwrap();
    assert!(matches!(fcm.send("gone").await, Err(SendError::TokenGone)));
    let apns = Apns::new(&ec_pem, "KEYID", "TEAM", true, http)
        .unwrap()
        .with_base(&base);
    apns.send("d1", "org.example.app", "Updates").await.unwrap();
    assert!(matches!(
        apns.send("gone", "org.example.app", "Updates").await,
        Err(SendError::TokenGone)
    ));

    let got = shared.0.lock().unwrap();
    assert_eq!(got.fcm.len(), 3);
    assert_eq!(
        got.fcm[0]["message"]["data"]["t"], "sync",
        "no content, only a wake-up"
    );
    assert_eq!(got.fcm[0]["message"]["android"]["priority"], "high");
    let (token, topic, body) = &got.apns[0];
    assert_eq!((token.as_str(), topic.as_str()), ("d1", "org.example.app"));
    assert_eq!(body["aps"]["alert"]["body"], "Updates");
    assert_eq!(body["aps"]["content-available"], 1);
}
