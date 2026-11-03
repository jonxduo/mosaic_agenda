//! Qualifications that exclude each other: granting one from a date closes
//! the other on that date, even if it was already closed later, so a
//! backdated grant never overlaps the one it replaces.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use core_api::{Core, CoreOptions};
use core_crypto::MemorySecretStore;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backdated_grant_replaces_the_one_it_excludes() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (p, _) = core_config::load_and_validate(&dir).unwrap();
    let bundle = core_config::compile(p).unwrap().bytes;
    let tmp = tempfile::tempdir().unwrap();
    let a = Core::open(CoreOptions {
        data_dir: tmp.path().join("a"),
        bundle,
        secrets: Arc::new(MemorySecretStore::default()),
        network: None,
        clock: None,
        pin_params: Some(core_crypto::pin::PinParams::insecure_for_tests()),
        server: None,
        security: None,
    })
    .unwrap();
    a.create_org("Club".into(), "Admin".into()).await.unwrap();
    let m = a
        .add_person(
            "Bob".into(),
            [("level".to_owned(), serde_json::json!(3))]
                .into_iter()
                .collect(),
        )
        .unwrap();
    // `junior` excludes `senior`.
    a.grant_qualification(m.clone(), "junior".into(), Some("2010-03-01".into()), None)
        .unwrap();
    // Junior closed today in one step…
    let junior = a
        .people()
        .unwrap()
        .persons
        .into_iter()
        .find(|x| x.person_id == m)
        .unwrap()
        .held
        .into_iter()
        .find(|h| h.qualification == "junior")
        .unwrap();
    a.end_grant("qualification".into(), junior.grant_id, None)
        .unwrap();
    // …senior from 2017 in the next: junior now ends in 2017.
    a.grant_qualification(m.clone(), "senior".into(), Some("2017-11-03".into()), None)
        .unwrap();
    let v = a.view().unwrap();
    let periods: Vec<(String, String, Option<String>)> = v
        .proj
        .auth
        .org
        .qualification_grants
        .iter()
        .filter(|g| g.person.to_string() == m && g.qualification.as_str() != "member")
        .map(|g| {
            (
                g.qualification.to_string(),
                g.period.start.to_string(),
                g.period.end.map(|e| e.to_string()),
            )
        })
        .collect();
    assert!(
        periods.contains(&(
            "junior".into(),
            "2010-03-01".into(),
            Some("2017-11-03".into())
        )),
        "{periods:?}"
    );
    assert!(
        periods.contains(&("senior".into(), "2017-11-03".into(), None)),
        "{periods:?}"
    );
}
