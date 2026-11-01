//! iroh transport (§9.1): one endpoint per device, keyed by its Ed25519 key.
//!
//! - `org-app/sync/1`: a [`Session`] over one bidirectional QUIC stream.
//!   Frames are length-prefixed postcard. The session ends when both sides
//!   are idle and each has received everything the other sent (`Idle`
//!   frames carry the counts).
//! - `org-app/pair/1`: pairing of a new device ([`crate::pairing`]).
//! - gossip: new local events are announced on the organization's topic;
//!   a device hearing of an unknown event syncs with whoever relayed it.

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use core_crypto::{DeviceIdentity, DeviceKeys, DevicePublicKey};
use core_events::{EventHash, EventStore, OrgId};
use core_model::PersonId;
use iroh::address_lookup::memory::MemoryLookup;
use iroh::endpoint::{Connection, RecvStream, SendStream, presets};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, RelayMode, SecretKey};
use iroh_gossip::api::{Event as GossipEvent, GossipSender};
use iroh_gossip::{Gossip, TopicId};
use n0_future::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::blobs::BlobStore;
use crate::pairing::{Invitations, PairRequest, PairResponse, Ticket};
use crate::protocol::{Msg, Peer, Session, SessionStats, SyncError};

pub const SYNC_ALPN: &[u8] = b"org-app/sync/1";
pub const PAIR_ALPN: &[u8] = b"org-app/pair/1";
const MAX_FRAME: usize = 4 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to try reaching a peer before giving up for this round.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Serialize, Deserialize)]
enum Frame {
    Data(Msg),
    Idle { sent: u64, recv: u64 },
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn t(e: impl std::fmt::Display) -> SyncError {
    SyncError::Transport(e.to_string())
}

/// Boxed future of a pairing decision.
pub type ConfirmFuture = std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>;

/// Decision on a pairing request, shown to the user with the verification
/// code (the future resolves when they confirm or refuse).
pub trait PairConfirm: Send + Sync {
    fn confirm(&self, person: PersonId, identity: DeviceIdentity, code: String) -> ConfirmFuture;
}

/// Accepts every request (tests).
pub struct AlwaysConfirm;

impl PairConfirm for AlwaysConfirm {
    fn confirm(&self, _: PersonId, _: DeviceIdentity, _: String) -> ConfirmFuture {
        Box::pin(async { true })
    }
}

/// Everything a node needs, shared with its protocol handlers.
pub struct NodeState {
    pub store: EventStore,
    pub blobs: Box<dyn BlobStore>,
    /// Blobs this device wants; refreshed by the app from its projection.
    pub wanted_blobs: BTreeSet<[u8; 32]>,
    /// Devices allowed to sync (`None` = any, for tests and first contact).
    pub allowed: Option<BTreeSet<DevicePublicKey>>,
    pub invitations: Invitations,
    /// Confirmed pairings, for the app to publish `device.added`.
    pub paired: Vec<(PersonId, DeviceIdentity)>,
}

pub type Shared = Arc<Mutex<NodeState>>;

/// How the endpoint reaches the network.
#[derive(Debug, Clone)]
pub enum Network {
    /// Loopback only, no relay, addresses given explicitly (tests).
    Local,
    /// n0's public relays (development).
    PublicRelays,
    /// Own relays (production, from the genesis).
    Relays(Vec<String>),
}

#[derive(Clone)]
struct SyncHandler {
    state: Shared,
    keys: Arc<DeviceKeys>,
}

#[derive(Clone)]
struct PairHandler {
    state: Shared,
    keys: Arc<DeviceKeys>,
    confirm: Arc<dyn PairConfirm>,
}

impl std::fmt::Debug for SyncHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SyncHandler")
    }
}

impl std::fmt::Debug for PairHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PairHandler")
    }
}

fn device_of(conn: &Connection) -> DevicePublicKey {
    DevicePublicKey(*conn.remote_id().as_bytes())
}

fn lock(state: &Shared) -> Result<std::sync::MutexGuard<'_, NodeState>, SyncError> {
    state
        .lock()
        .map_err(|_| SyncError::Protocol("node state poisoned"))
}

impl ProtocolHandler for SyncHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let remote = device_of(&conn);
        let allowed = lock(&self.state)
            .map(|s| s.allowed.as_ref().is_none_or(|a| a.contains(&remote)))
            .unwrap_or(false);
        if !allowed {
            conn.close(1u32.into(), b"unknown device");
            return Ok(());
        }
        let (send, recv) = conn.accept_bi().await?;
        let _ = run_session(self.state.clone(), self.keys.clone(), send, recv).await;
        conn.closed().await;
        Ok(())
    }
}

async fn write_frame(send: &mut SendStream, frame: &Frame) -> Result<(), SyncError> {
    let bytes = postcard::to_allocvec(frame).map_err(t)?;
    let len = u32::try_from(bytes.len()).map_err(t)?;
    send.write_all(&len.to_be_bytes()).await.map_err(t)?;
    send.write_all(&bytes).await.map_err(t)?;
    Ok(())
}

async fn read_frame(recv: &mut RecvStream) -> Result<Frame, SyncError> {
    let mut len = [0u8; 4];
    tokio::time::timeout(IO_TIMEOUT, recv.read_exact(&mut len))
        .await
        .map_err(t)?
        .map_err(t)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(SyncError::Protocol("frame too large"));
    }
    let mut buf = vec![0u8; len];
    tokio::time::timeout(IO_TIMEOUT, recv.read_exact(&mut buf))
        .await
        .map_err(t)?
        .map_err(t)?;
    postcard::from_bytes(&buf).map_err(t)
}

/// Runs a session over a stream pair; the writer runs in its own task so
/// that both directions flow even with large batches.
async fn run_session(
    state: Shared,
    keys: Arc<DeviceKeys>,
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<SessionStats, SyncError> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Frame>();
    let writer = tokio::spawn(async move {
        while let Some(f) = rx.recv().await {
            write_frame(&mut send, &f).await?;
        }
        send.finish().map_err(t)?;
        Ok::<(), SyncError>(())
    });
    let wanted = lock(&state)?.wanted_blobs.clone();
    let mut session = Session::new(wanted);
    let mut sent: u64 = 0;
    let mut received: u64 = 0;
    let start = {
        let mut g = lock(&state)?;
        let st = &mut *g;
        let mut peer = Peer {
            store: &mut st.store,
            blobs: st.blobs.as_mut(),
            keys: &keys,
            now: now_ms(),
        };
        session.start(&mut peer)?
    };
    for m in start {
        tx.send(Frame::Data(m)).map_err(t)?;
        sent += 1;
    }
    let mut last_idle: Option<(u64, u64)> = None;
    let mut peer_idle: Option<(u64, u64)> = None;
    loop {
        if last_idle != Some((sent, received)) {
            tx.send(Frame::Idle {
                sent,
                recv: received,
            })
            .map_err(t)?;
            last_idle = Some((sent, received));
        }
        if peer_idle == Some((received, sent)) {
            break;
        }
        match read_frame(&mut recv).await? {
            Frame::Idle { sent: s, recv: r } => peer_idle = Some((s, r)),
            Frame::Data(m) => {
                received += 1;
                let out = {
                    let mut g = lock(&state)?;
                    let st = &mut *g;
                    let mut peer = Peer {
                        store: &mut st.store,
                        blobs: st.blobs.as_mut(),
                        keys: &keys,
                        now: now_ms(),
                    };
                    session.handle(&mut peer, m)?
                };
                for m in out {
                    tx.send(Frame::Data(m)).map_err(t)?;
                    sent += 1;
                }
            }
        }
    }
    drop(tx);
    writer.await.map_err(t)??;
    Ok(session.stats)
}

impl ProtocolHandler for PairHandler {
    async fn accept(&self, conn: Connection) -> Result<(), AcceptError> {
        let remote = device_of(&conn);
        let (mut send, mut recv) = conn.accept_bi().await?;
        let response = match read_frame_pair(&mut recv).await {
            Ok(req) => {
                let me = self.keys.public();
                let checked = lock(&self.state)
                    .map(|mut s| s.invitations.accept(&req, &remote, &me, now_ms()));
                match checked {
                    Ok(Ok((person, code))) => {
                        if self
                            .confirm
                            .confirm(person, req.identity, code.clone())
                            .await
                        {
                            let org = lock(&self.state).ok().and_then(|mut s| {
                                s.paired.push((person, req.identity));
                                if let Some(a) = s.allowed.as_mut() {
                                    a.insert(req.identity.signing);
                                }
                                s.store.org()
                            });
                            match org {
                                Some(org) => PairResponse::Accepted { org },
                                None => PairResponse::Rejected {
                                    reason: "no organization".into(),
                                },
                            }
                        } else {
                            PairResponse::Rejected {
                                reason: "not confirmed".into(),
                            }
                        }
                    }
                    Ok(Err(e)) => PairResponse::Rejected {
                        reason: e.to_string(),
                    },
                    Err(e) => PairResponse::Rejected {
                        reason: e.to_string(),
                    },
                }
            }
            Err(e) => PairResponse::Rejected {
                reason: e.to_string(),
            },
        };
        if let Ok(bytes) = postcard::to_allocvec(&response) {
            let _ = send.write_all(&(bytes.len() as u32).to_be_bytes()).await;
            let _ = send.write_all(&bytes).await;
            let _ = send.finish();
        }
        conn.closed().await;
        Ok(())
    }
}

async fn read_frame_pair<T: serde::de::DeserializeOwned>(
    recv: &mut RecvStream,
) -> Result<T, SyncError> {
    let mut len = [0u8; 4];
    tokio::time::timeout(IO_TIMEOUT, recv.read_exact(&mut len))
        .await
        .map_err(t)?
        .map_err(t)?;
    let len = u32::from_be_bytes(len) as usize;
    if len > 64 * 1024 {
        return Err(SyncError::Protocol("pairing message too large"));
    }
    let mut buf = vec![0u8; len];
    tokio::time::timeout(IO_TIMEOUT, recv.read_exact(&mut buf))
        .await
        .map_err(t)?
        .map_err(t)?;
    postcard::from_bytes(&buf).map_err(t)
}

/// Gossip announcement of new events.
#[derive(Debug, Serialize, Deserialize)]
struct Announce {
    org: OrgId,
    events: Vec<(EventHash, u64)>,
}

pub fn gossip_topic(org: &OrgId) -> TopicId {
    let mut h = blake3::Hasher::new();
    h.update(b"gossip");
    h.update(&org.0);
    TopicId::from_bytes(*h.finalize().as_bytes())
}

/// A running device on the network.
pub struct Node {
    endpoint: Endpoint,
    router: Router,
    keys: Arc<DeviceKeys>,
    pub state: Shared,
    gossip: Gossip,
    lookup: MemoryLookup,
    sender: tokio::sync::Mutex<Option<GossipSender>>,
}

impl Node {
    pub async fn spawn(
        keys: Arc<DeviceKeys>,
        state: Shared,
        network: Network,
        confirm: Arc<dyn PairConfirm>,
    ) -> Result<Self, SyncError> {
        let secret = SecretKey::from_bytes(&keys.signing_secret());
        let lookup = MemoryLookup::new();
        let builder = match &network {
            Network::Local => Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Disabled)
                .bind_addr("127.0.0.1:0")
                .map_err(t)?,
            Network::PublicRelays => Endpoint::builder(presets::N0),
            Network::Relays(urls) => {
                let parsed: Vec<iroh::RelayUrl> =
                    urls.iter().filter_map(|u| u.parse().ok()).collect();
                Endpoint::builder(presets::N0)
                    .relay_mode(RelayMode::Custom(iroh::RelayMap::from_iter(parsed)))
            }
        };
        let endpoint = builder
            .secret_key(secret)
            .address_lookup(lookup.clone())
            .bind()
            .await
            .map_err(t)?;
        let gossip = Gossip::builder().spawn(endpoint.clone());
        let router = Router::builder(endpoint.clone())
            .accept(
                SYNC_ALPN,
                SyncHandler {
                    state: state.clone(),
                    keys: keys.clone(),
                },
            )
            .accept(
                PAIR_ALPN,
                PairHandler {
                    state: state.clone(),
                    keys: keys.clone(),
                    confirm,
                },
            )
            .accept(iroh_gossip::ALPN, gossip.clone())
            .spawn();
        Ok(Self {
            endpoint,
            router,
            keys,
            state,
            gossip,
            lookup,
            sender: tokio::sync::Mutex::new(None),
        })
    }

    pub fn device(&self) -> DevicePublicKey {
        self.keys.public()
    }

    pub fn keys(&self) -> &DeviceKeys {
        &self.keys
    }

    /// Address to give to peers (loopback sockets in `Local` mode).
    pub fn addr(&self) -> EndpointAddr {
        let mut addr = self.endpoint.addr();
        for s in self.endpoint.bound_sockets() {
            let s = if s.ip().is_unspecified() {
                SocketAddr::from(([127, 0, 0, 1], s.port()))
            } else {
                s
            };
            addr = addr.with_ip_addr(s);
        }
        addr
    }

    /// Teaches the endpoint where a peer is (tests, tickets with addresses).
    pub fn learn(&self, addr: EndpointAddr) {
        self.lookup.add_endpoint_info(addr);
    }

    fn endpoint_id(device: &DevicePublicKey) -> Result<iroh::EndpointId, SyncError> {
        iroh::PublicKey::from_bytes(&device.0).map_err(t)
    }

    /// Reconciles with a peer.
    pub async fn sync_with(&self, device: &DevicePublicKey) -> Result<SessionStats, SyncError> {
        let conn = tokio::time::timeout(
            CONNECT_TIMEOUT,
            self.endpoint.connect(Self::endpoint_id(device)?, SYNC_ALPN),
        )
        .await
        .map_err(t)?
        .map_err(t)?;
        let (send, recv) = conn.open_bi().await.map_err(t)?;
        let stats = run_session(self.state.clone(), self.keys.clone(), send, recv).await;
        conn.close(0u32.into(), b"done");
        stats
    }

    /// Joins an organization through a ticket. On success the local store is
    /// bound to the organization and has synced with the inviter.
    pub async fn pair(&self, ticket: &Ticket) -> Result<PairResponse, SyncError> {
        let mut addr = EndpointAddr::new(Self::endpoint_id(&ticket.inviter)?);
        for a in &ticket.addrs {
            if let Ok(s) = a.parse::<SocketAddr>() {
                addr = addr.with_ip_addr(s);
            }
        }
        self.learn(addr.clone());
        let conn = tokio::time::timeout(CONNECT_TIMEOUT, self.endpoint.connect(addr, PAIR_ALPN))
            .await
            .map_err(t)?
            .map_err(t)?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(t)?;
        let req = PairRequest {
            invite: ticket.invite,
            secret: ticket.secret,
            identity: self.keys.identity(),
        };
        let bytes = postcard::to_allocvec(&req).map_err(t)?;
        send.write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .map_err(t)?;
        send.write_all(&bytes).await.map_err(t)?;
        send.finish().map_err(t)?;
        let response: PairResponse = read_frame_pair(&mut recv).await?;
        conn.close(0u32.into(), b"done");
        if let PairResponse::Accepted { org } = &response {
            if *org != ticket.org {
                return Err(SyncError::WrongOrg);
            }
            lock(&self.state)?.store.bind_org(*org)?;
        }
        Ok(response)
    }

    /// Subscribes to the organization's gossip topic; unknown announced
    /// events trigger a sync with the device that relayed them.
    pub async fn start_gossip(
        self: &Arc<Self>,
        bootstrap: Vec<DevicePublicKey>,
    ) -> Result<(), SyncError> {
        let org = lock(&self.state)?
            .store
            .org()
            .ok_or(SyncError::Protocol("store without organization"))?;
        let peers = bootstrap
            .iter()
            .filter_map(|d| Self::endpoint_id(d).ok())
            .collect();
        let topic = self
            .gossip
            .subscribe(gossip_topic(&org), peers)
            .await
            .map_err(t)?;
        let (sender, mut receiver) = topic.split();
        *self.sender.lock().await = Some(sender);
        let node = Arc::downgrade(self);
        tokio::spawn(async move {
            while let Some(Ok(ev)) = receiver.next().await {
                let GossipEvent::Received(msg) = ev else {
                    continue;
                };
                let Some(node) = node.upgrade() else {
                    break;
                };
                let Ok(a) = postcard::from_bytes::<Announce>(&msg.content) else {
                    continue;
                };
                let unknown = lock(&node.state)
                    .map(|s| {
                        a.org == org
                            && a.events
                                .iter()
                                .any(|(id, _)| s.store.status(id).ok().flatten().is_none())
                    })
                    .unwrap_or(false);
                if unknown {
                    let from = DevicePublicKey(*msg.delivered_from.as_bytes());
                    let _ = node.sync_with(&from).await;
                }
            }
        });
        Ok(())
    }

    /// Announces new local events to the organization.
    pub async fn announce(&self, events: Vec<(EventHash, u64)>) -> Result<(), SyncError> {
        let org = lock(&self.state)?
            .store
            .org()
            .ok_or(SyncError::Protocol("store without organization"))?;
        let bytes = postcard::to_allocvec(&Announce { org, events }).map_err(t)?;
        if let Some(s) = self.sender.lock().await.as_mut() {
            s.broadcast(bytes.into()).await.map_err(t)?;
        }
        Ok(())
    }

    pub async fn shutdown(self) -> Result<(), SyncError> {
        self.router.shutdown().await.map_err(t)?;
        Ok(())
    }
}
