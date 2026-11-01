//! Pairing a new device (`org-app/pair/1`, §9.6).
//!
//! 1. The inviter publishes `device.invited` and shows a ticket (QR or
//!    `orgapp:` link) with the organization, its own address, the invite id,
//!    a one-time 32-byte secret and the expiry (15 minutes by default).
//! 2. The new device connects to the inviter (QUIC, authenticated by the
//!    inviter's key in the ticket) and presents the secret and its keys.
//! 3. Both show a 6-digit code derived from the secret and the keys; the
//!    inviter confirms, then publishes `device.added` and distributes keys.

use std::collections::BTreeMap;

use core_crypto::{DeviceIdentity, DevicePublicKey, random_array};
use core_events::OrgId;
use core_model::{Id, PersonId};
use data_encoding::BASE32_NOPAD;
use serde::{Deserialize, Serialize};

pub const TICKET_PREFIX: &str = "orgapp:";
/// Default validity of an invitation.
pub const DEFAULT_TTL_MS: i64 = 15 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub org: OrgId,
    pub inviter: DevicePublicKey,
    pub relay_url: Option<String>,
    /// Direct addresses of the inviter (`ip:port`), when known.
    pub addrs: Vec<String>,
    pub invite: Id,
    pub secret: [u8; 32],
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairError {
    #[error("this is not a valid invitation")]
    BadTicket,
    #[error("the invitation has expired")]
    Expired,
    #[error("unknown or already used invitation")]
    UnknownInvite,
    #[error("wrong invitation secret")]
    BadSecret,
    #[error("the connecting device does not match the presented keys")]
    IdentityMismatch,
    #[error("the inviter did not confirm the code")]
    Rejected,
    #[error("random generator failure")]
    Random,
}

impl Ticket {
    pub fn encode(&self) -> String {
        let bytes = postcard::to_allocvec(self).unwrap_or_default();
        format!(
            "{TICKET_PREFIX}{}",
            BASE32_NOPAD.encode(&bytes).to_ascii_lowercase()
        )
    }

    pub fn decode(s: &str) -> Result<Self, PairError> {
        let body = s
            .trim()
            .strip_prefix(TICKET_PREFIX)
            .ok_or(PairError::BadTicket)?;
        let bytes = BASE32_NOPAD
            .decode(body.to_ascii_uppercase().as_bytes())
            .map_err(|_| PairError::BadTicket)?;
        postcard::from_bytes(&bytes).map_err(|_| PairError::BadTicket)
    }
}

/// 6-digit code shown on both devices: `blake3(secret || keys)`.
pub fn verification_code(
    secret: &[u8; 32],
    inviter: &DevicePublicKey,
    joiner: &DeviceIdentity,
) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"org-app/pair-code/1");
    h.update(secret);
    h.update(&inviter.0);
    h.update(&joiner.signing.0);
    h.update(&joiner.exchange.0);
    let b = h.finalize();
    let n = u32::from_le_bytes([
        b.as_bytes()[0],
        b.as_bytes()[1],
        b.as_bytes()[2],
        b.as_bytes()[3],
    ]) % 1_000_000;
    format!("{n:06}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    pub invite: Id,
    pub secret: [u8; 32],
    pub identity: DeviceIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairResponse {
    /// Confirmed: the device has been added; sync with the inviter now.
    Accepted {
        org: OrgId,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub person: PersonId,
    pub secret: [u8; 32],
    pub expires_at: i64,
}

/// Open invitations of the inviter device (kept in memory: tickets are short-lived).
#[derive(Debug, Default)]
pub struct Invitations {
    open: BTreeMap<Id, Invite>,
}

fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Invitations {
    /// Creates an invitation for `person`; publish `device.invited` with the
    /// returned id and show the ticket.
    pub fn create(
        &mut self,
        org: OrgId,
        inviter: DevicePublicKey,
        person: PersonId,
        now: i64,
        relay_url: Option<String>,
        addrs: Vec<String>,
    ) -> Result<Ticket, PairError> {
        let invite = Id::from_bytes(random_array::<16>().map_err(|_| PairError::Random)?);
        let secret = random_array::<32>().map_err(|_| PairError::Random)?;
        let expires_at = now + DEFAULT_TTL_MS;
        self.open.insert(
            invite,
            Invite {
                person,
                secret,
                expires_at,
            },
        );
        Ok(Ticket {
            org,
            inviter,
            relay_url,
            addrs,
            invite,
            secret,
            expires_at,
        })
    }

    /// Checks a request from the device connected as `remote`. On success the
    /// invitation is consumed and the code to compare is returned.
    pub fn accept(
        &mut self,
        req: &PairRequest,
        remote: &DevicePublicKey,
        inviter: &DevicePublicKey,
        now: i64,
    ) -> Result<(PersonId, String), PairError> {
        if &req.identity.signing != remote {
            return Err(PairError::IdentityMismatch);
        }
        let invite = self.open.get(&req.invite).ok_or(PairError::UnknownInvite)?;
        if now > invite.expires_at {
            self.open.remove(&req.invite);
            return Err(PairError::Expired);
        }
        if !constant_time_eq(&invite.secret, &req.secret) {
            return Err(PairError::BadSecret);
        }
        let person = invite.person;
        self.open.remove(&req.invite);
        Ok((
            person,
            verification_code(&req.secret, inviter, &req.identity),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_crypto::DeviceKeys;

    fn keys() -> DeviceKeys {
        DeviceKeys::generate().unwrap_or_else(|_| DeviceKeys::from_secret_bytes(&[1; 32], &[2; 32]))
    }

    #[test]
    fn ticket_round_trip() {
        let mut inv = Invitations::default();
        let inviter = keys();
        let t = inv
            .create(
                OrgId([5; 32]),
                inviter.public(),
                PersonId(Id::from_bytes([1; 16])),
                1000,
                Some("https://relay.example".into()),
                vec!["127.0.0.1:1".into()],
            )
            .unwrap_or_else(|e| panic!("{e}"));
        let s = t.encode();
        assert!(s.starts_with("orgapp:"));
        assert_eq!(Ticket::decode(&s), Ok(t));
        assert_eq!(Ticket::decode("orgapp:zzzz"), Err(PairError::BadTicket));
        assert_eq!(Ticket::decode("http://x"), Err(PairError::BadTicket));
    }

    #[test]
    fn invitations_are_single_use_checked_and_expire() {
        let mut inv = Invitations::default();
        let inviter = keys();
        let joiner = keys();
        let person = PersonId(Id::from_bytes([1; 16]));
        let t = inv
            .create(OrgId([5; 32]), inviter.public(), person, 1000, None, vec![])
            .unwrap_or_else(|e| panic!("{e}"));
        let req = PairRequest {
            invite: t.invite,
            secret: t.secret,
            identity: joiner.identity(),
        };
        // Another device replaying the request.
        let thief = keys();
        assert_eq!(
            inv.accept(&req, &thief.public(), &inviter.public(), 1001),
            Err(PairError::IdentityMismatch)
        );
        let mut wrong = req.clone();
        wrong.secret[0] ^= 1;
        assert_eq!(
            inv.accept(&wrong, &joiner.public(), &inviter.public(), 1001),
            Err(PairError::BadSecret)
        );
        let (p, code) = inv
            .accept(&req, &joiner.public(), &inviter.public(), 1001)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p, person);
        assert_eq!(code.len(), 6);
        assert_eq!(
            code,
            verification_code(&t.secret, &inviter.public(), &joiner.identity())
        );
        assert_eq!(
            inv.accept(&req, &joiner.public(), &inviter.public(), 1002),
            Err(PairError::UnknownInvite)
        );
        let t2 = inv
            .create(OrgId([5; 32]), inviter.public(), person, 0, None, vec![])
            .unwrap_or_else(|e| panic!("{e}"));
        let req2 = PairRequest {
            invite: t2.invite,
            secret: t2.secret,
            identity: joiner.identity(),
        };
        assert_eq!(
            inv.accept(
                &req2,
                &joiner.public(),
                &inviter.public(),
                DEFAULT_TTL_MS + 1
            ),
            Err(PairError::Expired)
        );
    }

    #[test]
    fn code_depends_on_every_key() {
        let (a, b, c) = (keys(), keys(), keys());
        let s = [7; 32];
        let base = verification_code(&s, &a.public(), &b.identity());
        assert_ne!(base, verification_code(&s, &c.public(), &b.identity()));
        assert_ne!(base, verification_code(&s, &a.public(), &c.identity()));
        assert_ne!(
            base,
            verification_code(&[8; 32], &a.public(), &b.identity())
        );
    }
}
