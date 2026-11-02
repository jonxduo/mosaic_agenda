//! Wake-ups through a real push service (in this process, fake FCM/APNs):
//! the root administrator publishes the devices, a second device registers
//! its token, and events it has not confirmed make the app ask to wake it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use core_api::{AppState as UiState, Core, CoreOptions, NetworkParams};
use core_crypto::MemorySecretStore;
use core_crypto::pin::PinParams;
use core_push::Platform;
use push_service::db::{Db, Target};
use push_service::senders::{SendError, Sender};

#[derive(Default)]
struct FakeSender {
    sent: Mutex<Vec<String>>,
}

#[async_trait]
impl Sender for FakeSender {
    async fn send(&self, t: &Target, _notice: &str) -> Result<(), SendError> {
        self.sent.lock().unwrap().push(t.token.clone());
        Ok(())
    }
}

fn club_bundle() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    core_config::compile(pkg).unwrap().bytes
}

fn open(dir: &Path, name: &str, push_url: &str) -> Arc<Core> {
    Core::open(CoreOptions {
        data_dir: dir.join(name),
        bundle: club_bundle(),
        secrets: Arc::new(MemorySecretStore::default()),
        network: Some(core_sync::net::Network::Local),
        clock: None,
        pin_params: Some(PinParams::insecure_for_tests()),
        server: Some(NetworkParams {
            relay_urls: vec![],
            dns_url: None,
            push_url: Some(push_url.to_owned()),
        }),
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unconfirmed_events_wake_the_recipients() {
    let sender = Arc::new(FakeSender::default());
    let state = Arc::new(push_service::AppState {
        db: Mutex::new(Db::open_in_memory().unwrap()),
        limits: Mutex::new(Default::default()),
        sender: sender.clone(),
        default_notice: "x".into(),
        clock: core_push::now_ms,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, push_service::router(state))
            .await
            .unwrap()
    });

    let dir = tempfile::tempdir().unwrap();
    let a = open(dir.path(), "a", &url);
    let b = open(dir.path(), "b", &url);
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    // The genesis names the push service.
    assert_eq!(
        a.genesis_network().unwrap().push_url.as_deref(),
        Some(url.as_str())
    );
    assert!(a.publish_membership().await.unwrap(), "first publication");
    assert!(
        !a.publish_membership().await.unwrap(),
        "unchanged: not sent again"
    );

    let bob = a.add_person("Bob".into(), Default::default()).unwrap();
    let inv = a.invite_device(bob).await.unwrap();
    b.start_network().await.unwrap();
    let code = b.join_org(inv.ticket).await.unwrap();
    for _ in 0..100 {
        if a.pairings().unwrap().iter().any(|p| p.code == code) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    a.confirm_pairing(code, true).await.unwrap();
    for _ in 0..150 {
        if b.status().unwrap().state == UiState::Ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // The new device is published (here or already by the background
    // sync), then it can register its token.
    a.publish_membership().await.unwrap();
    b.register_push(Platform::Fcm, "token-b".into(), "org.example.club".into())
        .await
        .unwrap();

    // A new event that b has not confirmed yet: b is woken, once.
    a.add_person("Carol".into(), Default::default()).unwrap();
    assert_eq!(a.wake_missing().await.unwrap(), 1);
    assert_eq!(
        sender.sent.lock().unwrap().as_slice(),
        &["token-b".to_owned()]
    );
    assert_eq!(
        a.wake_missing().await.unwrap(),
        0,
        "coalesced on the client"
    );
}
