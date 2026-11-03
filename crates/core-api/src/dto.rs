//! Data exchanged with the interface. Labels are already resolved in the
//! package's language; ids are strings; dates are `YYYY-MM-DD`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppState {
    /// No organization on this device yet.
    Empty,
    /// The app is locked (PIN).
    Locked,
    /// Joined, waiting for the inviter to add this device.
    Joining,
    Ready,
    /// Local data erased after too many wrong PINs: restart the app.
    Wiped,
    /// A backup was restored: restart the app to use it.
    Restart,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Branding {
    pub app_name: String,
    pub primary_color: String,
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Me {
    pub person_id: String,
    pub display_name: String,
    pub device_id: String,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AppStatus {
    pub state: AppState,
    pub branding: Branding,
    pub org_name: Option<String>,
    pub me: Option<Me>,
    /// Local events not yet confirmed by two other devices.
    pub unsynced: u32,
    pub unsynced_label: String,
    pub has_pin: bool,
    /// Biometric unlock enabled on this device.
    pub biometric: bool,
    /// Privacy notice (Markdown) to accept at first start.
    pub privacy_notice: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MenuItem {
    pub label: String,
    pub view: String,
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Action {
    pub id: String,
    pub label: String,
    /// `none`, `optional` or `required`.
    pub note: String,
    pub primary: bool,
    /// Shown but not usable yet (e.g. forwarding an incomplete batch too early).
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DueDocument {
    pub form: String,
    pub subject_id: String,
    pub period: String,
    pub title: String,
    pub subtitle: String,
    pub urgent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AssignmentItem {
    pub program_id: String,
    pub program_type: String,
    pub slot: String,
    pub occurrence: u32,
    pub index: u32,
    pub title: String,
    pub subtitle: String,
    /// `pending`, `accepted`, `declined`.
    pub response: String,
    pub can_respond: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HeldResource {
    pub resource_id: String,
    pub title: String,
    pub subtitle: String,
    pub open_request: Option<OpenRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OpenRequest {
    pub request_id: String,
    pub label: String,
    pub due: String,
    pub answers: Vec<Choice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BoardItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub body: Vec<BoardLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BoardLine {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HomeSection {
    DocumentsDue {
        title: String,
        items: Vec<DueDocument>,
    },
    Assignments {
        title: String,
        items: Vec<AssignmentItem>,
    },
    Resources {
        title: String,
        items: Vec<HeldResource>,
    },
    Board {
        title: String,
        items: Vec<BoardItem>,
    },
    /// Work waiting for this user in the views they can reach (core).
    Work { title: String, items: Vec<WorkItem> },
}

/// Something to do for others: a batch to forward, missing documents of a
/// unit, an aggregate to prepare, alerts, devices waiting to join.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkItem {
    pub title: String,
    pub subtitle: String,
    /// View to open, with its parameters (as in the menu).
    pub view: String,
    pub params: BTreeMap<String, String>,
    pub action: String,
    pub urgent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Home {
    pub date: String,
    pub greeting: String,
    pub todo_title: String,
    pub sections: Vec<HomeSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Field {
    pub id: String,
    pub label: String,
    /// Field type of the core (`integer`, `duration`, `boolean`, `text`, `choice`, ...).
    pub kind: String,
    pub required: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub max_length: Option<u32>,
    pub options: Vec<Choice>,
    #[specta(type = specta_typescript::Unknown)]
    pub value: serde_json::Value,
}

/// Field values sent by the interface (`id → value`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct JsonMap(
    #[specta(type = BTreeMap<String, specta_typescript::Unknown>)]
    pub  BTreeMap<String, serde_json::Value>,
);

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FormView {
    pub document_id: Option<String>,
    pub form: String,
    pub subject_id: String,
    pub subject_name: String,
    pub period: String,
    pub title: String,
    pub subtitle: String,
    pub state: Option<String>,
    pub state_label: Option<String>,
    pub delivery: Option<String>,
    pub editable: bool,
    pub fields: Vec<Field>,
    pub actions: Vec<Action>,
    pub readers: String,
    pub history: Vec<HistoryLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HistoryLine {
    pub date: String,
    pub label: String,
    pub by: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DocumentRow {
    pub document_id: String,
    pub form: String,
    pub subject_id: String,
    pub subject_name: String,
    pub period: String,
    pub period_label: String,
    pub state_label: String,
    pub variant_label: String,
    pub delivery: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PersonRow {
    pub person_id: String,
    pub name: String,
    pub detail: String,
    pub document_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BatchView {
    pub form: String,
    pub unit_id: String,
    pub unit_name: String,
    pub period: String,
    pub period_label: String,
    pub received: Vec<PersonRow>,
    pub missing: Vec<PersonRow>,
    pub progress: String,
    pub done: Vec<DocumentRow>,
    pub actions: Vec<Action>,
    pub hint: Option<String>,
    pub can_fill_on_behalf: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkQueue {
    pub title: String,
    pub batches: Vec<BatchView>,
    /// Documents with individual steps (e.g. send back) for this user.
    pub documents: Vec<DocumentRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Measure {
    pub label: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MeasureGroup {
    pub label: String,
    pub measures: Vec<Measure>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AggregateView {
    pub aggregate: String,
    pub title: String,
    pub period: String,
    pub period_label: String,
    pub version: Option<u32>,
    pub distributed: bool,
    pub groups: Vec<MeasureGroup>,
    pub missing: Vec<String>,
    pub documents: Vec<DocumentRow>,
    pub can_generate: bool,
    pub can_distribute: bool,
    pub registered_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProgramRow {
    pub program_id: String,
    pub program_type: String,
    pub title: String,
    pub subtitle: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProgramList {
    pub types: Vec<Choice>,
    pub can_draft: Vec<String>,
    pub programs: Vec<ProgramRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SlotCell {
    pub occurrence: u32,
    pub slot: String,
    pub index: u32,
    pub label: String,
    pub assignee: Option<String>,
    pub assignee_name: Option<String>,
    pub response: String,
    pub candidates: Vec<Choice>,
    /// The slot changes at every event inside a group (see `ProgramGroup::days`).
    pub per_event: bool,
    /// The slot's extra fields (title, minutes…) with their values.
    pub details: Vec<Field>,
    /// The slot chosen together with this one (`pair_with`), shown inside it.
    pub partner: Option<String>,
    /// This cell is the partner of another: the editor shows it there.
    pub paired: bool,
}

/// The details of one slot written in the editor: field → text, an empty
/// text clears the field.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DetailChange {
    pub occurrence: u32,
    pub slot: String,
    pub index: u32,
    pub values: BTreeMap<String, String>,
}

/// One slot choice of the program editor (an empty `person` empties it).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SlotChange {
    pub occurrence: u32,
    pub slot: String,
    pub index: u32,
    pub person: String,
}

/// Events sharing the same persons (one event unless the program rotates
/// by week or month). Its cells carry the first occurrence.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProgramGroup {
    pub occurrence: u32,
    pub label: String,
    /// The dates of the group when it has more than one.
    pub detail: Option<String>,
    /// Its events, for the slots that change at every event.
    pub days: Vec<ProgramDay>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProgramDay {
    pub occurrence: u32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProgramEditor {
    pub program_id: String,
    pub program_type: String,
    pub title: String,
    pub status: String,
    pub occurrences: Vec<String>,
    pub groups: Vec<ProgramGroup>,
    pub cells: Vec<SlotCell>,
    pub can_edit: bool,
    /// The draft differs from what was last sent.
    pub unsent: bool,
    /// The type reads details from a page per event (`prefill`).
    pub can_prefill: bool,
    /// Nothing was prefilled or written yet: the editor reads the pages on opening.
    pub needs_prefill: bool,
    pub can_send: bool,
    pub can_publish: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResourceRow {
    pub resource_id: String,
    pub title: String,
    pub holder: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResourceList {
    pub resource_type: String,
    pub title: String,
    pub can_manage: bool,
    pub can_assign: bool,
    pub resources: Vec<ResourceRow>,
    pub attributes: Vec<Field>,
    pub candidates: Vec<Choice>,
    pub requests: Vec<Choice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ResourceDetail {
    pub resource_id: String,
    pub title: String,
    pub attributes: Vec<BoardLine>,
    pub holder: Option<String>,
    pub history: Vec<HistoryLine>,
    pub log_fields: Vec<Field>,
    pub is_holder: bool,
    pub can_assign: bool,
    pub can_request: bool,
    pub open_request: Option<OpenRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Grant {
    pub grant_id: String,
    pub label: String,
    pub period: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PersonView {
    pub person_id: String,
    pub name: String,
    pub attributes: Vec<BoardLine>,
    pub qualifications: Vec<Grant>,
    pub unit: Option<String>,
    pub roles: Vec<Grant>,
    pub devices: Vec<Choice>,
    /// Data erased on request (§14): kept only for the history.
    pub erased: bool,
    /// The row's summary (package `ui.people`): dates and attributes
    /// (an empty label for a choice, whose value says it all)…
    pub facts: Vec<BoardLine>,
    /// …and the qualifications shown as marks.
    pub marks: Vec<String>,
    /// Attribute values as stored, to fill the edit form.
    pub values: JsonMap,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct People {
    /// Attributes, units, devices and privacy rights.
    pub can_manage: bool,
    /// Granting and ending qualifications.
    pub can_qualify: bool,
    /// Granting roles.
    pub can_grant_roles: bool,
    pub persons: Vec<PersonView>,
    pub qualifications: Vec<Choice>,
    pub roles: Vec<Choice>,
    pub units: Vec<Choice>,
    pub unit_types: Vec<Choice>,
    pub attributes: Vec<Field>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct UnitView {
    pub unit_id: String,
    pub name: String,
    pub unit_type: String,
    pub members: Vec<String>,
    pub roles: Vec<BoardLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Alert {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PeerInfo {
    pub device_id: String,
    pub person: String,
    pub archive: bool,
    pub last_contact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncStatus {
    pub online: bool,
    pub relay: String,
    pub events: u32,
    pub outbox: u32,
    pub last_sync: Option<String>,
    pub peers: Vec<PeerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Invitation {
    pub ticket: String,
    pub qr_svg: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Pairing {
    pub person: String,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Notification {
    pub id: String,
    pub text: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Settings {
    pub auto_lock_minutes: u32,
    pub mobile_data_photos: bool,
    pub muted_categories: Vec<String>,
    pub categories: Vec<Choice>,
}
