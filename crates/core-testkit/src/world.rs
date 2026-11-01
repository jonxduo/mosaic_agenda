//! A simulated organization: people with devices sharing one event log.
//!
//! Every view is computed from the events with the keys of one device, as a
//! real device would. `concurrent` actions are all built from the same log
//! (as if their devices were offline) and appended together.

use std::collections::BTreeMap;

use core_crypto::{ChannelKeyring, DeviceKeys};
use core_engine::actions::{ActionError, Client};
use core_engine::channels::derive_keyring;
use core_engine::time::ms_of;
use core_engine::{Projection, rebuild};
use core_events::SignedEvent;
use core_events::genesis::{NewOrganization, create_genesis};
use core_events::payloads::{NetworkParams, RootAdmin, SecurityParams};
use core_model::{Id, PersonId, UnitId};
use time::Date;

pub const ADMIN: &str = "admin";

/// An action to run later as some person.
pub type BoxedAction = Box<dyn FnOnce(&Client<'_>) -> Result<Vec<SignedEvent>, ActionError>>;

#[derive(Debug, thiserror::Error)]
pub enum WorldError {
    #[error("unknown {0} `{1}`")]
    Unknown(&'static str, String),
    #[error("{0}")]
    Action(#[from] ActionError),
    #[error("engine: {0}")]
    Engine(String),
    #[error("{0}")]
    Other(String),
}

pub struct Device {
    pub keys: DeviceKeys,
    pub revoked: bool,
}

pub struct World {
    pub events: Vec<SignedEvent>,
    pub persons: BTreeMap<String, PersonId>,
    pub devices: BTreeMap<String, Vec<Device>>,
    pub units: BTreeMap<String, UnitId>,
    pub externals: BTreeMap<String, Id>,
    pub memberships: BTreeMap<String, Id>,
    pub role_grants: BTreeMap<(String, String), Id>,
    pub qualification_grants: BTreeMap<(String, String), Id>,
    pub programs: BTreeMap<String, Id>,
    pub resources: BTreeMap<String, Id>,
    pub requests: BTreeMap<String, Id>,
    pub last_document: BTreeMap<String, Id>,
    now: i64,
}

impl World {
    /// A new organization whose only root administrator is `admin`.
    pub fn new(bundle: Vec<u8>, date: Date) -> Result<Self, WorldError> {
        let keys = DeviceKeys::generate().map_err(|e| WorldError::Other(e.to_string()))?;
        let person = PersonId(Id::from_bytes(
            core_crypto::random_array::<16>().map_err(|e| WorldError::Other(e.to_string()))?,
        ));
        let org = NewOrganization {
            name: "Test".into(),
            root_admins: vec![RootAdmin {
                person,
                devices: vec![keys.identity()],
            }],
            config_bundle: bundle,
            network: NetworkParams {
                relay_urls: vec![],
                dns_url: None,
            },
            security: SecurityParams::default(),
        };
        let now = ms_of(date) + 8 * 3_600_000;
        let (_, genesis) =
            create_genesis(org, &keys, now).map_err(|e| WorldError::Other(e.to_string()))?;
        let mut w = Self {
            events: vec![genesis],
            persons: BTreeMap::from([(ADMIN.to_owned(), person)]),
            devices: BTreeMap::from([(
                ADMIN.to_owned(),
                vec![Device {
                    keys,
                    revoked: false,
                }],
            )]),
            units: BTreeMap::new(),
            externals: BTreeMap::new(),
            memberships: BTreeMap::new(),
            role_grants: BTreeMap::new(),
            qualification_grants: BTreeMap::new(),
            programs: BTreeMap::new(),
            resources: BTreeMap::new(),
            requests: BTreeMap::new(),
            last_document: BTreeMap::new(),
            now,
        };
        w.set_date(date);
        Ok(w)
    }

    /// Moves the clock to `date` (08:00 UTC); later actions are a second apart.
    pub fn set_date(&mut self, date: Date) {
        self.now = ms_of(date) + 8 * 3_600_000;
    }

    fn tick(&mut self) -> i64 {
        self.now += 1000;
        self.now
    }

    pub fn person(&self, name: &str) -> Result<PersonId, WorldError> {
        self.persons
            .get(name)
            .copied()
            .ok_or_else(|| WorldError::Unknown("person", name.to_owned()))
    }

    pub fn name_of(&self, person: PersonId) -> Option<&str> {
        self.persons
            .iter()
            .find(|(_, p)| **p == person)
            .map(|(n, _)| n.as_str())
    }

    pub fn device(&self, name: &str, index: Option<usize>) -> Result<&Device, WorldError> {
        let list = self
            .devices
            .get(name)
            .ok_or_else(|| WorldError::Unknown("devices of", name.to_owned()))?;
        match index {
            Some(i) => list
                .get(i)
                .ok_or_else(|| WorldError::Unknown("device", format!("{name}#{i}"))),
            None => list
                .iter()
                .find(|d| !d.revoked)
                .ok_or_else(|| WorldError::Unknown("active device of", name.to_owned())),
        }
    }

    /// The projection and keyring of a device.
    pub fn view_of(
        &self,
        events: &[SignedEvent],
        keys: &DeviceKeys,
    ) -> Result<(Projection, ChannelKeyring), WorldError> {
        let ring = derive_keyring(events, keys);
        let proj = rebuild(events, &ring).map_err(|e| WorldError::Engine(e.to_string()))?;
        Ok((proj, ring))
    }

    pub fn view(&self, name: &str, index: Option<usize>) -> Result<Projection, WorldError> {
        let d = self.device(name, index)?;
        Ok(self.view_of(&self.events, &d.keys)?.0)
    }

    /// Builds events as `name` (first active device) on `base` events.
    fn build<F>(
        &self,
        base: &[SignedEvent],
        name: &str,
        now: i64,
        f: F,
    ) -> Result<Vec<SignedEvent>, WorldError>
    where
        F: FnOnce(&Client<'_>) -> Result<Vec<SignedEvent>, ActionError>,
    {
        let d = self.device(name, None)?;
        let (proj, ring) = self.view_of(base, &d.keys)?;
        let client = Client {
            events: base,
            proj: &proj,
            keys: &d.keys,
            keyring: &ring,
            now,
        };
        Ok(f(&client)?)
    }

    /// Runs an action as `name` and appends its events.
    pub fn act<F>(&mut self, name: &str, f: F) -> Result<Vec<SignedEvent>, WorldError>
    where
        F: FnOnce(&Client<'_>) -> Result<Vec<SignedEvent>, ActionError>,
    {
        let now = self.tick();
        let events = self.events.clone();
        let new = self.build(&events, name, now, f)?;
        self.events.extend(new.iter().cloned());
        Ok(new)
    }

    /// Like [`act`](Self::act) for actions returning a value too.
    pub fn act_with<T, F>(&mut self, name: &str, f: F) -> Result<T, WorldError>
    where
        F: FnOnce(&Client<'_>) -> Result<(T, Vec<SignedEvent>), ActionError>,
    {
        let now = self.tick();
        let events = self.events.clone();
        let d = self.device(name, None)?;
        let (proj, ring) = self.view_of(&events, &d.keys)?;
        let client = Client {
            events: &events,
            proj: &proj,
            keys: &d.keys,
            keyring: &ring,
            now,
        };
        let (value, new) = f(&client)?;
        self.events.extend(new);
        Ok(value)
    }

    /// Several actions built from the same log, then appended together.
    pub fn concurrent(&mut self, actions: Vec<(String, BoxedAction)>) -> Result<(), WorldError> {
        let base = self.events.clone();
        let mut all = Vec::new();
        for (name, f) in actions {
            let now = self.tick();
            all.extend(self.build(&base, &name, now, f)?);
        }
        self.events.extend(all);
        Ok(())
    }

    /// Runs the key-completion task on every active device (§8.3).
    pub fn complete_channels(&mut self) -> Result<(), WorldError> {
        let names: Vec<String> = self.devices.keys().cloned().collect();
        for n in names {
            let count = self.devices.get(&n).map_or(0, Vec::len);
            for i in 0..count {
                if self
                    .devices
                    .get(&n)
                    .and_then(|l| l.get(i))
                    .is_some_and(|d| d.revoked)
                {
                    continue;
                }
                let now = self.tick();
                let events = self.events.clone();
                let d = self.device(&n, Some(i))?;
                let (proj, ring) = self.view_of(&events, &d.keys)?;
                if proj.auth.author(&d.keys.public()).is_err() {
                    continue;
                }
                let client = Client {
                    events: &events,
                    proj: &proj,
                    keys: &d.keys,
                    keyring: &ring,
                    now,
                };
                let new = client.complete_channels()?;
                self.events.extend(new);
            }
        }
        Ok(())
    }

    /// Adds a device to a person (by the admin) and completes the keys.
    pub fn add_device(&mut self, name: &str) -> Result<usize, WorldError> {
        let person = self.person(name)?;
        let keys = DeviceKeys::generate().map_err(|e| WorldError::Other(e.to_string()))?;
        let identity = keys.identity();
        self.act(ADMIN, |c| c.add_device(person, identity))?;
        let list = self.devices.entry(name.to_owned()).or_default();
        list.push(Device {
            keys,
            revoked: false,
        });
        let index = list.len() - 1;
        self.complete_channels()?;
        Ok(index)
    }

    pub fn revoke_device(&mut self, name: &str, index: usize) -> Result<(), WorldError> {
        let key = self.device(name, Some(index))?.keys.public();
        self.act(ADMIN, |c| c.revoke_device(key))?;
        if let Some(d) = self.devices.get_mut(name).and_then(|l| l.get_mut(index)) {
            d.revoked = true;
        }
        Ok(())
    }
}
