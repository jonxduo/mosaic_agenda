//! Concurrent operations on the same object (§7.6).
//!
//! Among concurrent events of a group (same document step target, same batch
//! step, same periodic document), the author with the highest role priority
//! wins, then the smallest event id. An event concurrent with a winner loses.

use std::collections::{BTreeMap, BTreeSet};

use core_model::config::Periodicity;

use crate::ops::Op;
use crate::projection::{Ctx, Projection};
use crate::time::date_of;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Step(core_model::Id),
    Batch(String),
    Document(String),
}

pub(crate) fn superseded(ctx: &mut Ctx<'_>, proj: &Projection) -> BTreeSet<usize> {
    let cfg = proj.cfg().cloned();
    let mut groups: BTreeMap<Key, Vec<usize>> = BTreeMap::new();
    for i in 0..ctx.graph.events.len() {
        let key = match &ctx.decoded[i].op {
            Some(Op::DocumentStep(s)) => Key::Step(s.document),
            Some(Op::BatchForwarded(b) | Op::BatchRegistered(b)) => Key::Batch(format!(
                "{}|{}|{}|{}",
                b.batch.form, b.batch.unit, b.batch.period, b.step
            )),
            Some(Op::DocumentCreated(c)) => {
                let periodic = cfg
                    .as_ref()
                    .and_then(|cfg| cfg.form(&c.form))
                    .is_some_and(|f| f.periodicity != Periodicity::None);
                if !periodic {
                    continue;
                }
                Key::Document(format!("{}|{}|{}", c.form, c.subject, c.period))
            }
            _ => continue,
        };
        groups.entry(key).or_default().push(i);
    }
    let mut losers = BTreeSet::new();
    for members in groups.values().filter(|m| m.len() > 1) {
        let mut ranked: Vec<(i64, core_events::EventHash, usize)> = members
            .iter()
            .map(|&i| {
                let e = &ctx.graph.events[i];
                let auth = ctx.auth_past(i, &proj.configs);
                let priority = match (auth.author(&e.body.author), cfg.as_ref()) {
                    (Ok(p), Some(cfg)) => auth.priority(cfg, p, date_of(e.body.declared_time)),
                    _ => i32::MIN,
                };
                (-i64::from(priority), e.id, i)
            })
            .collect();
        ranked.sort();
        let mut winners: Vec<usize> = Vec::new();
        for (_, _, i) in ranked {
            if winners.iter().any(|&w| ctx.graph.concurrent(w, i)) {
                losers.insert(i);
            } else {
                winners.push(i);
            }
        }
    }
    losers
}
