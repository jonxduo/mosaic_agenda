//! Pictures of photo fields are stored encrypted, each with its own key,
//! and open again from their reference; points open as map links; a photo
//! with no card in it is refused.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use core_api::{Core, CoreOptions};
use core_crypto::MemorySecretStore;
use core_sync::net::Network;
use data_encoding::BASE64;

fn club_bundle() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    core_config::compile(pkg).unwrap().bytes
}

fn core(dir: &Path) -> Arc<Core> {
    Core::open(CoreOptions {
        data_dir: dir.join("a"),
        bundle: club_bundle(),
        secrets: Arc::new(MemorySecretStore::default()),
        network: Some(Network::Local),
        clock: None,
        pin_params: Some(core_crypto::pin::PinParams::insecure_for_tests()),
        server: None,
        security: None,
    })
    .unwrap()
}

fn png_url(w: u32, h: u32, rgb: [u8; 3]) -> String {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    format!("data:image/png;base64,{}", BASE64.encode(&out))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pictures_and_points_of_a_resource() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    assert!(a.resources("tool".into()).unwrap().can_scan);

    let id = a
        .create_resource(
            "tool".into(),
            [
                ("name".to_owned(), serde_json::json!("Saw")),
                ("pic".to_owned(), serde_json::json!([png_url(40, 20, [200, 10, 10])])),
                ("spot".to_owned(), serde_json::json!("45.1,9.2")),
            ]
            .into_iter()
            .collect(),
        )
        .unwrap();
    let r = a.resource(id.clone()).unwrap();
    assert!(r.can_manage && r.can_scan);
    assert_eq!(r.pictures.len(), 1);
    let reference = r.pictures[0].reference.clone();
    assert!(!reference.starts_with("data:"), "stored as a reference");
    let url = a.attachment(reference.clone()).unwrap();
    let jpeg = BASE64
        .decode(url.trim_start_matches("data:image/jpeg;base64,").as_bytes())
        .unwrap();
    let back = image::load_from_memory(&jpeg).unwrap().to_rgb8();
    assert_eq!(back.dimensions(), (40, 20));
    assert!(back.get_pixel(20, 10)[0] > 150);
    // Pictures and points are not listed as text lines.
    assert!(r.attributes.iter().all(|l| l.label != "Picture" && l.label != "Spot"));
    assert_eq!(r.places.len(), 1);
    assert_eq!((r.places[0].lat, r.places[0].lng), (45.1, 9.2));
    assert!(r.places[0].link.contains("45.1,9.2"));

    // Editing keeps a stored picture as it is.
    let mut values: std::collections::BTreeMap<String, serde_json::Value> = r
        .fields
        .iter()
        .map(|f| (f.id.clone(), f.value.clone()))
        .collect();
    values.insert("name".into(), serde_json::json!("Big saw"));
    a.update_resource(id.clone(), values).unwrap();
    let r = a.resource(id).unwrap();
    assert!(r.title.contains("Big saw"));
    assert_eq!(r.pictures[0].reference, reference);

    // A point out of range and a forged reference are refused.
    assert!(
        a.create_resource(
            "tool".into(),
            [("spot".to_owned(), serde_json::json!("95,9"))].into_iter().collect()
        )
        .is_err()
    );
    assert!(a.attachment(format!("{}:{}", "00".repeat(32), "11".repeat(32))).is_err());

    // A photo of the table alone has no card to read.
    let err = a
        .scan_resource("tool".into(), png_url(300, 200, [90, 60, 40]))
        .await
        .unwrap_err();
    assert_eq!(err.kind, "unreadable");
}
