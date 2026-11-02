//! `push-service`: configuration from the environment (see
//! `server/deploy/.env.example` and docs/DEPLOY.md).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use push_service::db::Db;
use push_service::senders::{Apns, Dispatch, Fcm};
use push_service::{AppState, router};

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn read(path_var: &str) -> Result<Option<String>, String> {
    match env(path_var) {
        Some(p) => std::fs::read_to_string(&p)
            .map(Some)
            .map_err(|e| format!("{path_var}: {e}")),
        None => Ok(None),
    }
}

/// Keeps the last 7 daily copies of the database.
async fn backups(state: Arc<AppState>, dir: PathBuf) {
    loop {
        let day = core_push::now_ms() / 86_400_000;
        let path = dir.join(format!("push-{}.db", day % 7));
        if let Ok(db) = state.db.lock() {
            if let Err(e) = db.backup_to(&path) {
                eprintln!("backup failed: {e}");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(86_400)).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    core_push::ensure_tls_provider();
    let data = PathBuf::from(env("PUSH_DATA").unwrap_or_else(|| "/data".into()));
    std::fs::create_dir_all(&data)?;
    let listen = env("PUSH_LISTEN").unwrap_or_else(|| "0.0.0.0:8080".into());
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let fcm = match read("FCM_SERVICE_ACCOUNT_FILE")? {
        Some(json) => Some(Fcm::from_service_account(&json, http.clone())?),
        None => None,
    };
    let apns = match read("APNS_KEY_FILE")? {
        Some(p8) => Some(Apns::new(
            &p8,
            &env("APNS_KEY_ID").ok_or("APNS_KEY_ID is required with APNS_KEY_FILE")?,
            &env("APNS_TEAM_ID").ok_or("APNS_TEAM_ID is required with APNS_KEY_FILE")?,
            env("APNS_SANDBOX").is_some_and(|v| v == "1" || v == "true"),
            http,
        )?),
        None => None,
    };
    println!(
        "push-service: FCM {}, APNs {}",
        if fcm.is_some() { "on" } else { "off" },
        if apns.is_some() { "on" } else { "off" }
    );
    let state = Arc::new(AppState {
        db: Mutex::new(Db::open(&data.join("push.db"))?),
        limits: Mutex::new(Default::default()),
        sender: Arc::new(Dispatch { fcm, apns }),
        default_notice: env("PUSH_DEFAULT_TEXT").unwrap_or_else(|| "New updates".into()),
        clock: core_push::now_ms,
    });
    let backup_dir = data.join("backups");
    std::fs::create_dir_all(&backup_dir)?;
    tokio::spawn(backups(state.clone(), backup_dir));
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    println!("push-service listening on {listen}");
    axum::serve(listener, router(state)).await?;
    Ok(())
}
