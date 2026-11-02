//! Embeds the compiled package (`APP_PACKAGE_BUNDLE`, set by `cargo xtask build/dev`)
//! and makes sure a frontend exists so the crate compiles before the UI is built.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=APP_PACKAGE_BUNDLE");
    for v in ["APP_RELAY_URLS", "APP_DNS_URL", "APP_PUSH_URL"] {
        println!("cargo:rerun-if-env-changed={v}");
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_else(|_| ".".into()));
    let bundle = match std::env::var("APP_PACKAGE_BUNDLE") {
        Ok(path) => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::read(&path).unwrap_or_default()
        }
        Err(_) => Vec::new(),
    };
    if let Err(e) = std::fs::write(out.join("bundle.bin"), bundle) {
        println!("cargo:warning=cannot write the package bundle: {e}");
    }
    let dist = PathBuf::from("../ui/build");
    if !dist.join("index.html").exists() {
        let _ = std::fs::create_dir_all(&dist);
        let _ = std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>build the UI first</title><p>Run <code>cargo xtask dev</code>.</p>",
        );
    }
    tauri_build::build();
}
