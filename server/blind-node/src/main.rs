//! `blind-node`: configuration from the environment (docs/DEPLOY.md).
//!
//! - `BLIND_DATA`: data folder (default `/data`);
//! - `BLIND_ORG`: the organization's id (64 hex digits, shown in the app);
//! - `BLIND_RELAYS`: relay URLs, comma separated (empty: n0's public relays);
//! - `BLIND_DNS`: pkarr/DNS discovery URL (optional);
//! - `BLIND_QUOTA_GB`: space for attachments (default 5).

use std::path::PathBuf;
use std::time::Duration;

use blind_node::{BlindNode, Config, parse_org};
use core_sync::net::Network;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data = PathBuf::from(env("BLIND_DATA").unwrap_or_else(|| "/data".into()));
    let org = parse_org(&env("BLIND_ORG").ok_or("BLIND_ORG is required")?)?;
    let relays: Vec<String> = env("BLIND_RELAYS")
        .map(|r| {
            r.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let network = if relays.is_empty() {
        Network::PublicRelays
    } else {
        Network::Server {
            relays,
            dns: env("BLIND_DNS"),
        }
    };
    let quota_gb: u64 = env("BLIND_QUOTA_GB")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let node = BlindNode::start(Config {
        data,
        org,
        network,
        quota_bytes: quota_gb * 1024 * 1024 * 1024,
    })
    .await?;
    // The key to declare in the app (Sincronizzazione → nodo di backup).
    println!("blind node {}", node.id());
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        if let Err(e) = node.refresh() {
            eprintln!("refresh failed: {e}");
        }
    }
}
