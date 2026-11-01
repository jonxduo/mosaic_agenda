//! Delivery receipts (§9.2, §9.3): a device signs the ids of events it
//! received. Receipts are not events: they only travel between devices to
//! compute the delivery status of local events.

use core_crypto::{DeviceKeys, DevicePublicKey, Signature};
use core_events::EventHash;
use serde::{Deserialize, Serialize};

const CONTEXT: &[u8] = b"org-app/receipt/1";

fn message(event: &EventHash) -> Vec<u8> {
    let mut m = CONTEXT.to_vec();
    m.extend_from_slice(&event.0);
    m
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub event: EventHash,
    pub device: DevicePublicKey,
    pub signature: Signature,
}

impl Receipt {
    pub fn sign(event: EventHash, keys: &DeviceKeys) -> Self {
        Self {
            event,
            device: keys.public(),
            signature: keys.sign(&message(&event)),
        }
    }

    pub fn verify(&self) -> bool {
        self.device
            .verify(&message(&self.event), &self.signature)
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipts_are_bound_to_event_and_device() {
        let k = DeviceKeys::generate().unwrap_or_else(|_| unreachable!());
        let r = Receipt::sign(EventHash([1; 32]), &k);
        assert!(r.verify());
        let mut other_event = r.clone();
        other_event.event = EventHash([2; 32]);
        assert!(!other_event.verify());
        let mut other_device = r.clone();
        other_device.device = DeviceKeys::generate()
            .unwrap_or_else(|_| unreachable!())
            .public();
        assert!(!other_device.verify());
    }
}
