//! Content of every event kind, as the engine reads it.
//!
//! Public kinds use the payloads of `core-events`; the others are defined
//! here and travel encrypted. Each struct is postcard-encoded, so fields are
//! never removed or reordered (append-only, like event kinds).

use std::collections::BTreeMap;

use core_events::payloads::{
    DeviceAdded, DeviceInvited, DeviceRevoked, GenesisPayload, KeyDistribution,
};
use core_events::{EventHash, EventKind};
use core_model::{Id, PersonId, Scalar, Slug, UnitId};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use time::Date;

use crate::time::PeriodKey;

pub type Values = BTreeMap<Slug, Scalar>;

// ----- organization ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrgSettingsChanged {
    /// Root administrators added (person and their devices are added separately).
    pub add_root_admins: Vec<PersonId>,
    pub remove_root_admins: Vec<PersonId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigPublished {
    pub version: u32,
    /// `core-config` bundle bytes.
    pub bundle: Vec<u8>,
}

/// The organization's blind backup node (PROGETTO.md §10.3): a server peer
/// without channel keys that keeps every event and attachment, encrypted.
/// `None` removes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupNodeSet {
    pub node: Option<core_crypto::DevicePublicKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceArchiveSet {
    pub device: core_crypto::DevicePublicKey,
    pub archive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineResolved {
    pub event: EventHash,
    pub accept: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonAdded {
    pub person: PersonId,
    pub display_name: String,
    pub attributes: Values,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonUpdated {
    pub person: PersonId,
    pub display_name: Option<String>,
    /// Attributes set (others unchanged).
    pub attributes: Values,
    pub cleared: Vec<Slug>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonRemoved {
    pub person: PersonId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalPersonUpserted {
    pub id: Id,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualificationGranted {
    pub grant: Id,
    pub person: PersonId,
    pub qualification: Slug,
    pub start: Date,
    pub end: Option<Date>,
}

/// `qualification.ended`, `membership.ended`, `role.ended`: the grant stops
/// being valid from `end` (exclusive bound).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantEnded {
    pub grant: Id,
    pub end: Date,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitCreated {
    pub unit: UnitId,
    pub unit_type: Slug,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitUpdated {
    pub unit: UnitId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitClosed {
    pub unit: UnitId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipStarted {
    pub membership: Id,
    pub person: PersonId,
    pub unit: UnitId,
    pub start: Date,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleGranted {
    pub grant: Id,
    pub person: PersonId,
    pub role: Slug,
    /// Unit for unit-scoped roles.
    pub unit: Option<UnitId>,
    pub start: Date,
    pub end: Option<Date>,
}

// ----- documents ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentContent {
    pub document: Id,
    pub form: Slug,
    pub form_version: u32,
    pub subject: PersonId,
    pub period: PeriodKey,
    pub variant: Slug,
    /// Unit of the batching type the subject belongs to at the end of the period.
    pub unit: Option<UnitId>,
    pub values: Values,
    /// Created by someone else on the subject's behalf: the draft is then
    /// readable by `fillers_on_behalf` too.
    pub on_behalf: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentUpdated {
    pub document: Id,
    pub values: Values,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentStep {
    pub document: Id,
    pub step: Slug,
    pub note: Option<String>,
}

/// A document as known at a state, re-encrypted for a new audience (§8.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentSnapshot {
    pub content: DocumentContent,
    pub state: Slug,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Distributed {
    Document(DocumentSnapshot),
    Aggregate(AggregateSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchKey {
    pub form: Slug,
    pub unit: UnitId,
    pub period: PeriodKey,
}

impl PartialOrd for BatchKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BatchKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.form, &self.unit, &self.period).cmp(&(&other.form, &other.unit, &other.period))
    }
}

/// `batch.forwarded` and `batch.registered`: the step applies to these documents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchStep {
    pub batch: BatchKey,
    pub step: Slug,
    pub documents: Vec<Id>,
}

// ----- aggregates ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AggregateResult {
    /// Group key (e.g. `variant=x,unit=<id>`; empty = total) → measure → value.
    pub groups: BTreeMap<String, BTreeMap<Slug, f64>>,
    pub missing: Vec<PersonId>,
    pub documents: Vec<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AggregateSnapshot {
    pub aggregate: Slug,
    pub period: PeriodKey,
    pub version: u32,
    pub result: AggregateResult,
    pub note: Option<String>,
    /// Individual documents when `include_individual_documents`.
    pub documents: Vec<DocumentSnapshot>,
}

// ----- calendar and programs ------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarEventUpserted {
    pub event: Id,
    pub event_type: Slug,
    pub date: Date,
    pub time: Option<String>,
    pub location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarEventCancelled {
    pub event: Id,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SlotKey {
    /// Index in the program's occurrences.
    pub occurrence: u16,
    pub slot: Slug,
    /// Repetition × count position.
    pub index: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Assignee {
    Person(PersonId),
    External(Id),
    /// A whole unit (e.g. a group): its members hear of it.
    Unit(core_model::UnitId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssignmentSpec {
    pub key: SlotKey,
    pub assignee: Assignee,
    pub extra: Values,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramDrafted {
    pub program: Id,
    pub program_type: Slug,
    pub config_version: u32,
    /// Dates of the occurrences covered (events, shifts).
    pub occurrences: Vec<Date>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramUpdated {
    pub program: Id,
    pub assignments: Vec<AssignmentSpec>,
    pub cleared: Vec<SlotKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticeItem {
    pub key: SlotKey,
    pub date: Date,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AssignmentsSent {
    /// For the program team and those allowed to post it on the board: the program moves to
    /// `assigned`; the snapshot is what may be published.
    Team {
        program: Id,
        snapshot: ProgramSnapshot,
    },
    /// For one assignee, on their assignment channel.
    Notice {
        program: Id,
        program_type: Slug,
        person: PersonId,
        items: Vec<NoticeItem>,
    },
    /// For the team and who publishes, when assignees hear of their slots
    /// only at publication: who holds what in the version just sent, so
    /// that who publishes can write the notices without the draft.
    Roster {
        program: Id,
        version: u32,
        persons: Vec<(PersonId, Vec<NoticeItem>)>,
    },
}

/// The details of slots (their extra fields, e.g. the title of a part),
/// set with or without an assignee. Empty values clear a slot's details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramDetailsSet {
    pub program: Id,
    pub details: Vec<(SlotKey, Values)>,
}

/// Days added to a program for its dedicated slots (§ "Giorno dedicato"):
/// they follow its events in the occurrence numbering, in the order added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramDaysAdded {
    pub program: Id,
    pub days: Vec<Date>,
}

/// Parts of a program shown or removed for one event (`true`: shown).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramPartsShown {
    pub program: Id,
    pub parts: Vec<(SlotKey, bool)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Accepted,
    Declined { note: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentResponded {
    pub program: Id,
    pub program_type: Slug,
    pub key: SlotKey,
    pub assignee: Assignee,
    pub response: Response,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishedAssignment {
    pub key: SlotKey,
    pub date: Date,
    pub display_name: String,
    pub extra: Values,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgramSnapshot {
    pub program: Id,
    pub program_type: Slug,
    pub version: u32,
    pub occurrences: Vec<Date>,
    pub assignments: Vec<PublishedAssignment>,
}

// ----- resources ------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceUpserted {
    pub resource: Id,
    pub resource_type: Slug,
    pub attributes: Values,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceAssigned {
    pub resource: Id,
    pub resource_type: Slug,
    pub holder: PersonId,
    pub date: Date,
    /// Attributes, so that the holder can see the resource.
    pub attributes: Values,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceLogged {
    pub resource: Id,
    pub resource_type: Slug,
    pub holder: PersonId,
    pub date: Date,
    pub values: Values,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRequestOpened {
    pub request: Id,
    pub resource: Id,
    pub resource_type: Slug,
    pub holder: PersonId,
    pub request_type: Slug,
    pub due: Date,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRequestAnswered {
    pub request: Id,
    pub resource: Id,
    pub resource_type: Slug,
    pub holder: PersonId,
    pub answer: Slug,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceReturned {
    pub resource: Id,
    pub resource_type: Slug,
    pub holder: PersonId,
    pub date: Date,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRetired {
    pub resource: Id,
    pub resource_type: Slug,
}

// ----- board ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardPosted {
    pub post: Id,
    pub audience: Slug,
    pub title: String,
    pub body: String,
    pub from: Date,
    pub until: Date,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardWithdrawn {
    pub post: Id,
    pub audience: Slug,
}

// ----- privacy ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErasureRequested {
    pub person: PersonId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionApplied {
    pub before: Date,
}

/// Decoded content of an event.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Genesis(Box<GenesisPayload>),
    OrgSettingsChanged(OrgSettingsChanged),
    ConfigPublished(ConfigPublished),
    DeviceInvited(DeviceInvited),
    DeviceAdded(DeviceAdded),
    DeviceRevoked(DeviceRevoked),
    DeviceArchiveSet(DeviceArchiveSet),
    BackupNodeSet(BackupNodeSet),
    PersonAdded(PersonAdded),
    PersonUpdated(PersonUpdated),
    PersonRemoved(PersonRemoved),
    ExternalPersonUpserted(ExternalPersonUpserted),
    QualificationGranted(QualificationGranted),
    QualificationEnded(GrantEnded),
    UnitCreated(UnitCreated),
    UnitUpdated(UnitUpdated),
    UnitClosed(UnitClosed),
    MembershipStarted(MembershipStarted),
    MembershipEnded(GrantEnded),
    RoleGranted(RoleGranted),
    RoleEnded(GrantEnded),
    KeysDistributed(KeyDistribution),
    ChannelRotated(KeyDistribution),
    DocumentCreated(DocumentContent),
    DocumentUpdated(DocumentUpdated),
    DocumentStep(DocumentStep),
    DocumentDistributed(Box<Distributed>),
    BatchForwarded(BatchStep),
    BatchRegistered(BatchStep),
    AggregateGenerated(Box<AggregateSnapshot>),
    CalendarEventUpserted(CalendarEventUpserted),
    CalendarEventCancelled(CalendarEventCancelled),
    ProgramDrafted(ProgramDrafted),
    ProgramUpdated(ProgramUpdated),
    ProgramDetailsSet(ProgramDetailsSet),
    ProgramPartsShown(ProgramPartsShown),
    ProgramDaysAdded(ProgramDaysAdded),
    ProgramAssignmentsSent(AssignmentsSent),
    AssignmentResponded(AssignmentResponded),
    ProgramPublished(ProgramSnapshot),
    ProgramRevised(ProgramSnapshot),
    ResourceUpserted(ResourceUpserted),
    ResourceAssigned(ResourceAssigned),
    ResourceLogged(ResourceLogged),
    ResourceRequestOpened(ResourceRequestOpened),
    ResourceRequestAnswered(ResourceRequestAnswered),
    ResourceReturned(ResourceReturned),
    ResourceRetired(ResourceRetired),
    BoardPosted(BoardPosted),
    BoardWithdrawn(BoardWithdrawn),
    ErasureRequested(ErasureRequested),
    RetentionApplied(RetentionApplied),
    QuarantineResolved(QuarantineResolved),
}

fn de<T: DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}

impl Op {
    /// Decodes the clear content of an event of `kind`.
    pub fn decode(kind: EventKind, bytes: &[u8]) -> Option<Op> {
        use EventKind as K;
        Some(match kind {
            K::OrgGenesis => Op::Genesis(Box::new(de(bytes)?)),
            K::OrgSettingsChanged => Op::OrgSettingsChanged(de(bytes)?),
            K::ConfigPublished => Op::ConfigPublished(de(bytes)?),
            K::DeviceInvited => Op::DeviceInvited(de(bytes)?),
            K::DeviceAdded => Op::DeviceAdded(de(bytes)?),
            K::DeviceRevoked => Op::DeviceRevoked(de(bytes)?),
            K::DeviceArchiveSet => Op::DeviceArchiveSet(de(bytes)?),
            K::PersonAdded => Op::PersonAdded(de(bytes)?),
            K::PersonUpdated => Op::PersonUpdated(de(bytes)?),
            K::PersonRemoved => Op::PersonRemoved(de(bytes)?),
            K::ExternalPersonUpserted => Op::ExternalPersonUpserted(de(bytes)?),
            K::QualificationGranted => Op::QualificationGranted(de(bytes)?),
            K::QualificationEnded => Op::QualificationEnded(de(bytes)?),
            K::UnitCreated => Op::UnitCreated(de(bytes)?),
            K::UnitUpdated => Op::UnitUpdated(de(bytes)?),
            K::UnitClosed => Op::UnitClosed(de(bytes)?),
            K::MembershipStarted => Op::MembershipStarted(de(bytes)?),
            K::MembershipEnded => Op::MembershipEnded(de(bytes)?),
            K::RoleGranted => Op::RoleGranted(de(bytes)?),
            K::RoleEnded => Op::RoleEnded(de(bytes)?),
            K::ChannelKeysDistributed => Op::KeysDistributed(de(bytes)?),
            K::ChannelRotated => Op::ChannelRotated(de(bytes)?),
            K::DocumentCreated => Op::DocumentCreated(de(bytes)?),
            K::DocumentUpdated => Op::DocumentUpdated(de(bytes)?),
            K::DocumentStep => Op::DocumentStep(de(bytes)?),
            K::DocumentDistributed => Op::DocumentDistributed(Box::new(de(bytes)?)),
            K::BatchForwarded => Op::BatchForwarded(de(bytes)?),
            K::BatchRegistered => Op::BatchRegistered(de(bytes)?),
            K::AggregateGenerated => Op::AggregateGenerated(Box::new(de(bytes)?)),
            K::CalendarEventUpserted => Op::CalendarEventUpserted(de(bytes)?),
            K::CalendarEventCancelled => Op::CalendarEventCancelled(de(bytes)?),
            K::ProgramDrafted => Op::ProgramDrafted(de(bytes)?),
            K::ProgramUpdated => Op::ProgramUpdated(de(bytes)?),
            K::ProgramDetailsSet => Op::ProgramDetailsSet(de(bytes)?),
            K::ProgramPartsShown => Op::ProgramPartsShown(de(bytes)?),
            K::ProgramAssignmentsSent => Op::ProgramAssignmentsSent(de(bytes)?),
            K::AssignmentResponded => Op::AssignmentResponded(de(bytes)?),
            K::ProgramPublished => Op::ProgramPublished(de(bytes)?),
            K::ProgramRevised => Op::ProgramRevised(de(bytes)?),
            K::ResourceUpserted => Op::ResourceUpserted(de(bytes)?),
            K::ResourceAssigned => Op::ResourceAssigned(de(bytes)?),
            K::ResourceLogged => Op::ResourceLogged(de(bytes)?),
            K::ResourceRequestOpened => Op::ResourceRequestOpened(de(bytes)?),
            K::ResourceRequestAnswered => Op::ResourceRequestAnswered(de(bytes)?),
            K::ResourceReturned => Op::ResourceReturned(de(bytes)?),
            K::ResourceRetired => Op::ResourceRetired(de(bytes)?),
            K::BoardPosted => Op::BoardPosted(de(bytes)?),
            K::BoardWithdrawn => Op::BoardWithdrawn(de(bytes)?),
            K::PersonDataErasureRequested => Op::ErasureRequested(de(bytes)?),
            K::RetentionApplied => Op::RetentionApplied(de(bytes)?),
            K::AdminQuarantineResolved => Op::QuarantineResolved(de(bytes)?),
            K::OrgBackupNodeSet => Op::BackupNodeSet(de(bytes)?),
            K::ProgramDaysAdded => Op::ProgramDaysAdded(de(bytes)?),
        })
    }

    /// Kinds that change who may do what: they are validated on their own
    /// causal past and folded into the authorization state.
    pub fn is_authorization(kind: EventKind) -> bool {
        use EventKind as K;
        matches!(
            kind,
            K::OrgGenesis
                | K::OrgSettingsChanged
                | K::ConfigPublished
                | K::DeviceAdded
                | K::DeviceRevoked
                | K::DeviceArchiveSet
                | K::OrgBackupNodeSet
                | K::PersonAdded
                | K::PersonUpdated
                | K::PersonRemoved
                | K::ExternalPersonUpserted
                | K::QualificationGranted
                | K::QualificationEnded
                | K::UnitCreated
                | K::UnitUpdated
                | K::UnitClosed
                | K::MembershipStarted
                | K::MembershipEnded
                | K::RoleGranted
                | K::RoleEnded
        )
    }
}

/// Serializes a payload for an event.
pub fn encode<T: Serialize>(v: &T) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(v)
}
