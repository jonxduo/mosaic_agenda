//! The simulator on the neutral fixture (§15.3).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use core_testkit::sim::{SimOptions, parse_date, run};

const POPULATION: &str = r#"
groups: 2
ios_share: 0.5
profiles:
  - { weight: 3, qualifications: [member], attributes: { level: 1 } }
  - { weight: 1, qualifications: [member, senior], attributes: { level: 3 } }
roles:
  - { role: lead, per_unit: true }
  - { role: keeper }
archive_roles: [keeper]
"#;

#[test]
fn small_organization_meets_the_delivery_target() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    let bundle = core_config::compile(pkg).unwrap().bytes;
    let opts = SimOptions {
        devices: 10,
        days: 40,
        seed: 7,
        start: parse_date("2026-09-01").unwrap(),
    };
    let report = run(bundle, POPULATION, &opts).unwrap();
    println!("{report}");
    assert_eq!(report.persons, 10);
    assert_eq!(report.archives, 1);
    assert!(report.activity_events > 50, "{report}");
    assert!(
        report
            .kinds
            .iter()
            .any(|k| k.kind == "document.distributed" && k.pairs > 0),
        "{report}"
    );
    assert!(report.meets_target(), "{report}");
    assert_eq!(report.invalid, 0, "{report}");
}
