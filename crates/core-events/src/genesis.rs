//! The first event of an organization (PROGETTO.md §7.2).

use core_crypto::{DeviceKeys, random_array};
use core_model::PersonId;

use crate::error::EventError;
use crate::event::{Draft, SignedEvent};
use crate::hash::OrgId;
use crate::kind::EventKind;
use crate::payloads::{GenesisPayload, NetworkParams, RootAdmin, SecurityParams};

/// Input for a new organization.
#[derive(Debug, Clone)]
pub struct NewOrganization {
    pub name: String,
    pub root_admins: Vec<RootAdmin>,
    pub config_bundle: Vec<u8>,
    pub network: NetworkParams,
    pub security: SecurityParams,
}

/// Outcome of [`check_genesis`]: the payload and non-blocking warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisInfo {
    pub payload: GenesisPayload,
    /// e.g. only one root administrator (2 recommended).
    pub warnings: Vec<&'static str>,
}

/// Creates and signs the genesis. `keys` must be a device of a root admin.
pub fn create_genesis(
    org: NewOrganization,
    keys: &DeviceKeys,
    declared_time: i64,
) -> Result<(OrgId, SignedEvent), EventError> {
    let payload = GenesisPayload {
        name: org.name,
        root_admins: org.root_admins,
        config_hash: *blake3::hash(&org.config_bundle).as_bytes(),
        config_bundle: org.config_bundle,
        network: org.network,
        security: org.security,
        nonce: random_array::<16>()?,
    };
    let draft = Draft::after(
        OrgId::GENESIS_PLACEHOLDER,
        EventKind::OrgGenesis,
        &[],
        declared_time,
    );
    let event = draft.sign_public(&payload, keys)?;
    check_genesis(&event)?;
    Ok((event.org_id(), event))
}

/// Checks a genesis beyond the envelope: config hash, root admins, author.
pub fn check_genesis(event: &SignedEvent) -> Result<GenesisInfo, EventError> {
    if !event.is_genesis() {
        return Err(EventError::Genesis("not a genesis event"));
    }
    event.verify()?;
    let payload: GenesisPayload = event.public_content()?;
    if *blake3::hash(&payload.config_bundle).as_bytes() != payload.config_hash {
        return Err(EventError::Genesis("configuration hash mismatch"));
    }
    if payload.root_admins.is_empty() {
        return Err(EventError::Genesis(
            "at least one root administrator is required",
        ));
    }
    let mut persons: Vec<PersonId> = payload.root_admins.iter().map(|a| a.person).collect();
    persons.sort_unstable();
    persons.dedup();
    if persons.len() != payload.root_admins.len() {
        return Err(EventError::Genesis("a root administrator is listed twice"));
    }
    if payload.root_admins.iter().any(|a| a.devices.is_empty()) {
        return Err(EventError::Genesis(
            "every root administrator needs a device",
        ));
    }
    let author_is_root = payload
        .root_admins
        .iter()
        .flat_map(|a| a.devices.iter())
        .any(|d| d.signing == event.body.author);
    if !author_is_root {
        return Err(EventError::Genesis(
            "the author must be a root administrator's device",
        ));
    }
    if !(1..=30).contains(&payload.security.auto_lock_minutes) {
        return Err(EventError::Genesis(
            "auto_lock_minutes must be between 1 and 30",
        ));
    }
    let mut warnings = Vec::new();
    if payload.root_admins.len() < 2 {
        warnings.push("only one root administrator: at least two are recommended");
    }
    Ok(GenesisInfo { payload, warnings })
}
