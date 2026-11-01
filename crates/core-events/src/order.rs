//! Deterministic total order of events (PROGETTO.md §7.5).
//!
//! Every event's depth is 1 + the maximum depth of its parents, so sorting by
//! `(depth, id)` is a topological order of the graph with a deterministic
//! tie-break: every device applies the same events in the same order,
//! whatever order they arrived in.

use crate::event::SignedEvent;
use crate::hash::EventHash;

pub fn order_key(e: &SignedEvent) -> (u64, EventHash) {
    (e.body.depth, e.id)
}

pub fn sort_events(events: &mut [SignedEvent]) {
    events.sort_by_key(order_key);
}
