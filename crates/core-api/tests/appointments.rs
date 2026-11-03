//! Appointments that exclude each other: granting one from a date closes
//! the other on that date, even if it was already closed later, so a
//! backdated appointment never overlaps the one it replaces.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use core_api::{Core, CoreOptions};
use core_crypto::MemorySecretStore;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backdated_appointment_replaces_the_one_it_excludes() {
    let pkg = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/congregazione");
    let (p, _) = core_config::load_and_validate(&pkg).unwrap();
    let bundle = core_config::compile(p).unwrap().bytes;
    let dir = tempfile::tempdir().unwrap();
    let a = Core::open(CoreOptions {
        data_dir: dir.path().join("a"),
        bundle,
        secrets: Arc::new(MemorySecretStore::default()),
        network: None,
        clock: None,
        pin_params: Some(core_crypto::pin::PinParams::insecure_for_tests()),
        server: None,
        security: None,
    })
    .unwrap();
    a.create_org("C".into(), "Admin".into()).await.unwrap();
    let m = a
        .add_person(
            "Marco".into(),
            [("sex".to_owned(), serde_json::json!("m"))]
                .into_iter()
                .collect(),
        )
        .unwrap();
    for (q, f) in [
        ("publisher", "2000-01-01"),
        ("baptized", "2006-04-22"),
        ("ministerial_servant", "2010-03-01"),
    ] {
        a.grant_qualification(m.clone(), q.into(), Some(f.into()), None)
            .unwrap();
    }
    // Servant closed today in one save…
    let ms = a
        .people()
        .unwrap()
        .persons
        .into_iter()
        .find(|x| x.person_id == m)
        .unwrap()
        .held
        .into_iter()
        .find(|h| h.qualification == "ministerial_servant")
        .unwrap();
    a.end_grant("qualification".into(), ms.grant_id, None)
        .unwrap();
    // …elder from 2017 in the next: the servant now ends in 2017.
    a.grant_qualification(m.clone(), "elder".into(), Some("2017-11-03".into()), None)
        .unwrap();
    let v = a.view().unwrap();
    let ends: Vec<(String, String, Option<String>)> = v
        .proj
        .auth
        .org
        .qualification_grants
        .iter()
        .filter(|g| {
            g.person.to_string() == m
                && ["elder", "ministerial_servant"].contains(&g.qualification.as_str())
        })
        .map(|g| {
            (
                g.qualification.to_string(),
                g.period.start.to_string(),
                g.period.end.map(|e| e.to_string()),
            )
        })
        .collect();
    assert!(
        ends.contains(&(
            "ministerial_servant".into(),
            "2010-03-01".into(),
            Some("2017-11-03".into())
        )),
        "{ends:?}"
    );
    assert!(
        ends.contains(&("elder".into(), "2017-11-03".into(), None)),
        "{ends:?}"
    );
}
