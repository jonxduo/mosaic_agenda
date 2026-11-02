//! Wake-up limits (§10.1): at most 60 requests per sender and 10 pushes per
//! recipient per hour; requests for the same recipient within 2 minutes are
//! merged into one push.

use std::collections::{HashMap, VecDeque};

pub const SENDER_PER_HOUR: usize = 60;
pub const RECIPIENT_PER_HOUR: usize = 10;
pub const COALESCE_MS: i64 = 2 * 60 * 1000;
const HOUR_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Send,
    Coalesced,
    Limited,
}

#[derive(Default)]
pub struct Limits {
    senders: HashMap<String, VecDeque<i64>>,
    recipients: HashMap<String, VecDeque<i64>>,
}

fn prune(q: &mut VecDeque<i64>, now: i64) {
    while q.front().is_some_and(|t| now - t >= HOUR_MS) {
        q.pop_front();
    }
}

impl Limits {
    /// Counts one wake request of `sender`; false when over the limit.
    pub fn allow_sender(&mut self, sender: &str, now: i64) -> bool {
        let q = self.senders.entry(sender.to_owned()).or_default();
        prune(q, now);
        if q.len() >= SENDER_PER_HOUR {
            return false;
        }
        q.push_back(now);
        true
    }

    /// Whether to push `recipient` now (and records it when so).
    pub fn recipient(&mut self, recipient: &str, now: i64) -> Decision {
        let q = self.recipients.entry(recipient.to_owned()).or_default();
        prune(q, now);
        if q.back().is_some_and(|last| now - last < COALESCE_MS) {
            return Decision::Coalesced;
        }
        if q.len() >= RECIPIENT_PER_HOUR {
            return Decision::Limited;
        }
        q.push_back(now);
        Decision::Send
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sender_limit_per_hour() {
        let mut l = Limits::default();
        for i in 0..SENDER_PER_HOUR {
            assert!(l.allow_sender("a", i as i64));
        }
        assert!(!l.allow_sender("a", 100));
        assert!(l.allow_sender("b", 100));
        assert!(l.allow_sender("a", HOUR_MS + 1));
    }

    #[test]
    fn recipient_coalescing_and_limit() {
        let mut l = Limits::default();
        assert_eq!(l.recipient("r", 0), Decision::Send);
        assert_eq!(l.recipient("r", 60_000), Decision::Coalesced);
        let mut t = 0;
        for _ in 1..RECIPIENT_PER_HOUR {
            t += COALESCE_MS;
            assert_eq!(l.recipient("r", t), Decision::Send);
        }
        assert_eq!(l.recipient("r", t + COALESCE_MS), Decision::Limited);
        assert_eq!(l.recipient("r", HOUR_MS + COALESCE_MS * 2), Decision::Send);
    }
}
