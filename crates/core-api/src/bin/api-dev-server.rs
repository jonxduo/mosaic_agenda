//! Development bridge: serves the core's commands over HTTP on 127.0.0.1 so
//! the UI can run in a browser (and Playwright can test it) against a real
//! core. Never for production: secrets live in a plain file.
//!
//! `api-dev-server --data <dir> --bundle <bundle.bin> [--port 8787] [--network local|public]`

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use core_api::secrets::FileSecretStore;
use core_api::{Core, CoreOptions};
use core_sync::net::Network;

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

async fn call(
    State(core): State<Arc<Core>>,
    Path(cmd): Path<String>,
    body: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    let input = body.map(|Json(v)| v).unwrap_or(serde_json::Value::Null);
    match core_api::dispatch::dispatch(&core, &cmd, input).await {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::to_value(e).unwrap_or_default()),
        )
            .into_response(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = PathBuf::from(arg("--data").ok_or("--data <dir> is required")?);
    let bundle = std::fs::read(arg("--bundle").ok_or("--bundle <file> is required")?)?;
    let port: u16 = arg("--port")
        .map(|p| p.parse())
        .transpose()?
        .unwrap_or(8787);
    let network = match arg("--network").as_deref() {
        Some("public") => Some(Network::PublicRelays),
        Some("local") => Some(Network::Local),
        _ => None,
    };
    std::fs::create_dir_all(&data)?;
    let secrets = Arc::new(FileSecretStore::open(data.join("secrets.dev.json")));
    let core = Core::open(CoreOptions {
        data_dir: data,
        bundle,
        secrets,
        network,
        clock: None,
    })?;
    if core.has_org() {
        core.start_network().await?;
    }
    let sync_core = core.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            let _ = sync_core.sync_round().await;
        }
    });
    let app = Router::new()
        .route("/api/{cmd}", post(call))
        .with_state(core);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("api-dev-server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
