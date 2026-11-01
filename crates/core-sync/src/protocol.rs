//! Reconciliation of two event stores (`org-app/sync/1`, §9.2).
//!
//! 1. Both sides send a summary of their events by depth band
//!    (count and XOR of the ids of each band).
//! 2. For every band that differs, both send the ids they have there.
//! 3. Each side sends the events the other lacks, oldest first, in batches
//!    (at most 256 events or 1 MB per message).
//! 4. Received events are inserted (verified) and acknowledged with signed
//!    receipts; parents still missing are asked for explicitly.
//! 5. Wanted attachment blobs are requested and sent, checked by hash.
//!
//! [`Session`] is a reactive state machine: the transport only carries
//! messages, so the same code runs over iroh and in memory.

use std::collections::{BTreeMap, BTreeSet};

use core_crypto::DeviceKeys;
use core_events::{EventHash, EventStore, Inserted, OrgId, SignedEvent, StoreError};
use serde::{Deserialize, Serialize};

use crate::blobs::BlobStore;
use crate::receipt::Receipt;

/// Depths per band in the summary.
pub const BAND: u64 = 64;
pub const MAX_EVENTS_PER_MESSAGE: usize = 256;
pub const MAX_BYTES_PER_MESSAGE: usize = 1024 * 1024;
const MAX_IDS_PER_MESSAGE: usize = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandSummary {
    pub band: u64,
    pub count: u32,
    pub xor: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Msg {
    Hello {
        org: OrgId,
        summary: Vec<BandSummary>,
    },
    BandIds {
        bands: Vec<(u64, Vec<EventHash>)>,
    },
    Events {
        raw: Vec<Vec<u8>>,
    },
    Want {
        ids: Vec<EventHash>,
    },
    Receipts {
        receipts: Vec<Receipt>,
    },
    BlobWant {
        hashes: Vec<[u8; 32]>,
    },
    Blob {
        hash: [u8; 32],
        data: Vec<u8>,
    },
    /// The peer belongs to another organization.
    WrongOrg,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the peer belongs to another organization")]
    WrongOrg,
    #[error("blob store: {0}")]
    Blob(String),
    #[error("transport: {0}")]
    Transport(String),
    #[error("protocol: {0}")]
    Protocol(&'static str),
}

/// Summary of a store's events by depth band.
pub fn summarize(ids: &[(EventHash, u64)]) -> Vec<BandSummary> {
    let mut bands: BTreeMap<u64, BandSummary> = BTreeMap::new();
    for (id, depth) in ids {
        let band = depth / BAND;
        let b = bands.entry(band).or_insert(BandSummary {
            band,
            count: 0,
            xor: [0; 32],
        });
        b.count += 1;
        for (x, y) in b.xor.iter_mut().zip(id.0.iter()) {
            *x ^= y;
        }
    }
    bands.into_values().collect()
}

/// Statistics of a session, for logs and the simulator.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionStats {
    pub events_received: usize,
    pub events_new: usize,
    pub events_sent: usize,
    pub receipts_received: usize,
    pub blobs_received: usize,
    pub blobs_sent: usize,
}

/// What a session works on: borrowed for each message only.
pub struct Peer<'a> {
    pub store: &'a mut EventStore,
    pub blobs: &'a mut dyn BlobStore,
    pub keys: &'a DeviceKeys,
    pub now: i64,
}

/// State of one reconciliation with one peer.
#[derive(Debug, Default)]
pub struct Session {
    /// Blobs this device wants (computed by the caller from what it can read).
    wanted_blobs: BTreeSet<[u8; 32]>,
    asked: BTreeSet<EventHash>,
    pub stats: SessionStats,
    /// Events new to this store, in arrival order.
    pub new_events: Vec<EventHash>,
}

impl Session {
    pub fn new(wanted_blobs: BTreeSet<[u8; 32]>) -> Self {
        Self {
            wanted_blobs,
            ..Self::default()
        }
    }

    pub fn start(&mut self, p: &mut Peer<'_>) -> Result<Vec<Msg>, SyncError> {
        let org = p
            .store
            .org()
            .ok_or(SyncError::Protocol("store without organization"))?;
        let summary = summarize(&p.store.ids_with_depth()?);
        let mut out = vec![Msg::Hello { org, summary }];
        let missing: Vec<[u8; 32]> = self
            .wanted_blobs
            .iter()
            .filter(|h| !p.blobs.contains(h))
            .copied()
            .collect();
        if !missing.is_empty() {
            out.push(Msg::BlobWant { hashes: missing });
        }
        Ok(out)
    }

    fn events_message(
        &mut self,
        p: &mut Peer<'_>,
        ids: &[EventHash],
    ) -> Result<Vec<Msg>, SyncError> {
        let mut events: Vec<(u64, EventHash, Vec<u8>)> = Vec::new();
        for id in ids {
            if let Some(raw) = p.store.raw(id)? {
                let depth = SignedEvent::from_bytes(&raw)
                    .map(|e| e.body.depth)
                    .unwrap_or(u64::MAX);
                events.push((depth, *id, raw));
            }
        }
        events.sort_by_key(|e| (e.0, e.1));
        let mut out = Vec::new();
        let mut batch: Vec<Vec<u8>> = Vec::new();
        let mut bytes = 0;
        for (_, _, raw) in events {
            if !batch.is_empty()
                && (batch.len() >= MAX_EVENTS_PER_MESSAGE
                    || bytes + raw.len() > MAX_BYTES_PER_MESSAGE)
            {
                out.push(Msg::Events {
                    raw: std::mem::take(&mut batch),
                });
                bytes = 0;
            }
            bytes += raw.len();
            batch.push(raw);
            self.stats.events_sent += 1;
        }
        if !batch.is_empty() {
            out.push(Msg::Events { raw: batch });
        }
        Ok(out)
    }

    pub fn handle(&mut self, p: &mut Peer<'_>, msg: Msg) -> Result<Vec<Msg>, SyncError> {
        match msg {
            Msg::WrongOrg => Err(SyncError::WrongOrg),
            Msg::Hello { org, summary } => {
                if Some(org) != p.store.org() {
                    return Ok(vec![Msg::WrongOrg]);
                }
                let ids = p.store.ids_with_depth()?;
                let mine: BTreeMap<u64, BandSummary> =
                    summarize(&ids).into_iter().map(|b| (b.band, b)).collect();
                let theirs: BTreeMap<u64, BandSummary> =
                    summary.into_iter().map(|b| (b.band, b)).collect();
                let differing: BTreeSet<u64> = mine
                    .keys()
                    .chain(theirs.keys())
                    .filter(|b| mine.get(b) != theirs.get(b))
                    .copied()
                    .collect();
                if differing.is_empty() {
                    return Ok(Vec::new());
                }
                let mut bands: BTreeMap<u64, Vec<EventHash>> =
                    differing.iter().map(|b| (*b, Vec::new())).collect();
                for (id, depth) in ids {
                    if let Some(v) = bands.get_mut(&(depth / BAND)) {
                        v.push(id);
                    }
                }
                // Chunk large id lists.
                let mut out = Vec::new();
                let mut chunk: Vec<(u64, Vec<EventHash>)> = Vec::new();
                let mut n = 0;
                for (band, list) in bands {
                    if n + list.len() > MAX_IDS_PER_MESSAGE && !chunk.is_empty() {
                        out.push(Msg::BandIds {
                            bands: std::mem::take(&mut chunk),
                        });
                        n = 0;
                    }
                    n += list.len();
                    chunk.push((band, list));
                }
                out.push(Msg::BandIds { bands: chunk });
                Ok(out)
            }
            Msg::BandIds { bands } => {
                let ids = p.store.ids_with_depth()?;
                let band_set: BTreeSet<u64> = bands.iter().map(|(b, _)| *b).collect();
                let theirs: BTreeSet<EventHash> = bands.into_iter().flat_map(|(_, v)| v).collect();
                let lacking: Vec<EventHash> = ids
                    .into_iter()
                    .filter(|(id, d)| band_set.contains(&(d / BAND)) && !theirs.contains(id))
                    .map(|(id, _)| id)
                    .collect();
                self.events_message(p, &lacking)
            }
            Msg::Events { raw } => {
                let mut receipts = Vec::new();
                for r in raw {
                    self.stats.events_received += 1;
                    let Ok(event) = SignedEvent::from_bytes(&r) else {
                        continue;
                    };
                    match p.store.insert(&event, p.now) {
                        Ok(Inserted::New(_)) => {
                            self.stats.events_new += 1;
                            self.new_events.push(event.id);
                            receipts.push(Receipt::sign(event.id, p.keys));
                        }
                        Ok(Inserted::Duplicate) => receipts.push(Receipt::sign(event.id, p.keys)),
                        // Events of another organization or malformed: ignored.
                        Err(StoreError::WrongOrg | StoreError::Event(_)) => {}
                        Err(e) => return Err(e.into()),
                    }
                }
                let mut out = Vec::new();
                if !receipts.is_empty() {
                    out.push(Msg::Receipts { receipts });
                }
                let missing: Vec<EventHash> = p
                    .store
                    .missing()?
                    .into_iter()
                    .filter(|m| self.asked.insert(*m))
                    .collect();
                if !missing.is_empty() {
                    out.push(Msg::Want { ids: missing });
                }
                Ok(out)
            }
            Msg::Want { ids } => self.events_message(p, &ids),
            Msg::Receipts { receipts } => {
                for r in receipts.iter().filter(|r| r.verify()) {
                    self.stats.receipts_received += 1;
                    p.store.add_receipt(&r.event, &r.device, p.now)?;
                }
                Ok(Vec::new())
            }
            Msg::BlobWant { hashes } => {
                let mut out = Vec::new();
                for h in hashes {
                    if let Some(data) = p.blobs.get(&h).map_err(SyncError::Blob)? {
                        self.stats.blobs_sent += 1;
                        out.push(Msg::Blob { hash: h, data });
                    }
                }
                Ok(out)
            }
            Msg::Blob { hash, data } => {
                if *blake3::hash(&data).as_bytes() == hash && self.wanted_blobs.contains(&hash) {
                    p.blobs.put(&hash, &data).map_err(SyncError::Blob)?;
                    p.store
                        .set_blob(&hash, data.len() as u64, "present", None)?;
                    self.stats.blobs_received += 1;
                }
                Ok(Vec::new())
            }
        }
    }
}

/// Runs two sessions against each other in memory until both are quiet.
pub fn sync_in_memory(
    a: &mut Session,
    pa: &mut Peer<'_>,
    b: &mut Session,
    pb: &mut Peer<'_>,
) -> Result<(), SyncError> {
    let mut to_b = a.start(pa)?;
    let mut to_a = b.start(pb)?;
    let mut rounds = 0;
    while !to_a.is_empty() || !to_b.is_empty() {
        rounds += 1;
        if rounds > 10_000 {
            return Err(SyncError::Protocol("sync did not terminate"));
        }
        let mut next_b = Vec::new();
        for m in to_a.drain(..) {
            next_b.extend(a.handle(pa, m)?);
        }
        let mut next_a = Vec::new();
        for m in to_b.drain(..) {
            next_a.extend(b.handle(pb, m)?);
        }
        to_b = next_b;
        to_a = next_a;
    }
    Ok(())
}
