#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use core_crypto::DeviceKeys;
use core_events::genesis::{NewOrganization, create_genesis};
use core_events::payloads::{NetworkParams, RootAdmin, SecurityParams};
use core_events::{Draft, EventHash, EventKind, OrgId, SignedEvent};
use core_model::{Id, PersonId};

pub fn person(n: u8) -> PersonId {
    PersonId(Id::from_bytes([n; 16]))
}

pub fn org_with(admins: &[(&DeviceKeys, u8)]) -> NewOrganization {
    NewOrganization {
        name: "Test".into(),
        root_admins: admins
            .iter()
            .map(|(k, n)| RootAdmin {
                person: person(*n),
                devices: vec![k.identity()],
            })
            .collect(),
        config_bundle: b"compiled config".to_vec(),
        network: NetworkParams {
            relay_urls: vec![],
            dns_url: None,
            push_url: None,
        },
        security: SecurityParams::default(),
    }
}

pub fn genesis(keys: &DeviceKeys) -> (OrgId, SignedEvent) {
    create_genesis(org_with(&[(keys, 1)]), keys, 0).unwrap()
}

/// A small public event on top of `parents`.
pub fn child(keys: &DeviceKeys, org: OrgId, parents: &[(EventHash, u64)], t: i64) -> SignedEvent {
    Draft::after(org, EventKind::OrgSettingsChanged, parents, t)
        .sign_public(&t, keys)
        .unwrap()
}
