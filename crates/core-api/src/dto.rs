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
    /// The program type's icon; none for a notice.
    pub icon: Option<String>,
    pub body: Vec<BoardLine>,
    /// The programs shown: one, or those of a publish group.
    pub programs: Vec<String>,
    /// A publish group as one table: dates by rows, assignments by columns.
    pub grid: Option<BoardGrid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BoardGrid {
    pub columns: Vec<String>,
    /// Background of each column's heading: its program type's colour.
    pub colors: Vec<Option<String>>,
    pub rows: Vec<BoardGridRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BoardGridRow {
    /// The dates of the row, one per line: one event, or those sharing the
    /// persons (a week).
    pub dates: Vec<String>,
    /// For each column, the persons, one per line.
    pub cells: Vec<Vec<String>>,
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
    /// The form's name and its period apart, for a page heading.
    pub label: String,
    pub period_label: String,
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
    /// The answers, label and value, in the form's order.
    pub values: Vec<BoardLine>,
    /// Still in its initial state: it can be changed.
    pub editable: bool,
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

/// The units one oversees: members and documents month by month.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct UnitReports {
    pub title: String,
    pub units: Vec<UnitPanel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct UnitPanel {
    pub unit_id: String,
    pub unit_name: String,
    /// Members today, read only.
    pub members: Vec<PersonView>,
    /// Collections in progress (received, missing, steps), as in the work queue.
    pub batches: Vec<BatchView>,
    /// Periods newest first: the one being collected, then those with
    /// documents. Each with its totals and documents.
    pub months: Vec<MonthReports>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MonthReports {
    pub period: String,
    pub period_label: String,
    pub received: u32,
    pub expected: u32,
    /// Sums of the numeric fields, count of the yes answers.
    pub totals: Vec<Measure>,
    pub rows: Vec<DocumentRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Measure {
    pub label: String,
    pub value: f64,
    /// The value as shown (durations in hours, `1285 h`).
    pub display: String,
    /// Heading of the rows it belongs to.
    pub section: Option<String>,
    pub strong: bool,
    /// A cell of a table (row and column labels), a box, or a note.
    pub row: Option<String>,
    pub column: Option<String>,
    pub tile: bool,
    pub note: bool,
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
    /// The program type's icon.
    pub icon: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub status: String,
    /// A publish group shown as one program (`program_id` empty).
    pub group: Option<String>,
    /// The program's period (`YYYY-MM` or a week).
    pub period: String,
    /// A group missing one of its program types.
    pub incomplete: bool,
}

/// A publish group for one period: its programs handed in, those missing.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProgramGroupView {
    pub group: String,
    pub title: String,
    pub period: String,
    pub period_label: String,
    pub status: String,
    pub parts: Vec<ProgramGroupPart>,
    /// Program types not handed in yet (their names).
    pub missing: Vec<String>,
    pub can_publish: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProgramGroupPart {
    pub program_id: String,
    pub program_type: String,
    pub label: String,
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
    /// The slot whose row shows this cell (itself for a row of its own).
    pub row: String,
    /// The option whose column shows this cell; none for the main column.
    pub column: Option<String>,
    /// Section of the slot (`ProgramEditor::sections`).
    pub section: Option<String>,
    /// Takes a person (else it only holds details).
    pub assignable: bool,
    /// Part of this event (else removed, or not added yet).
    pub shown: bool,
    /// The editor shows the slot's name on the left of its row.
    pub row_name: bool,
    /// Word before the name under another person in the row (e.g. "con").
    pub prefix: Option<String>,
}

/// A person in the editor's side list, with how long since their last
/// slot in a program of the same type.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PersonLoad {
    pub id: String,
    pub name: String,
    /// Whole weeks from the last slot before this program; none if never.
    pub weeks: Option<u32>,
    /// Date (YYYY-MM-DD) of that last slot in another program of the type.
    pub last: Option<String>,
}

/// A slot a person holds in another program sent to who publishes
/// (published or not), for the editor's side list.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OtherSlot {
    pub person: String,
    /// YYYY-MM-DD.
    pub date: String,
    /// Program and slot, e.g. "Audio/video · Mixer audio".
    pub label: String,
    /// The slot alone, e.g. "Mixer audio".
    pub slot: String,
}

/// A heading of the program editor.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SectionView {
    pub id: String,
    pub label: String,
    pub color: Option<String>,
}

/// A part shown or removed in one event (editor).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PartChange {
    pub occurrence: u32,
    pub slot: String,
    pub index: u32,
    pub shown: bool,
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
    /// The same events as dates (YYYY-MM-DD).
    pub dates: Vec<String>,
    pub groups: Vec<ProgramGroup>,
    pub cells: Vec<SlotCell>,
    pub sections: Vec<SectionView>,
    /// Extra columns (the options on that have slots beside others).
    pub columns: Vec<Choice>,
    /// Everyone, the longest without a slot of this type first (for who edits).
    pub people: Vec<PersonLoad>,
    /// Slots of the people in the other programs sent (for who edits).
    pub others: Vec<OtherSlot>,
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
    /// Rows of the dedicated days (slots in that mode): date, slot, person.
    pub dedicated: Vec<DedicatedRow>,
    /// The slots one may add a dedicated row for, with who can take them.
    pub dedicated_slots: Vec<DedicatedSlot>,
    /// Slots go to units (groups), not persons: no people list beside.
    pub assign_units: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DedicatedRow {
    pub occurrence: u32,
    pub slot: String,
    pub index: u32,
    pub label: String,
    /// YYYY-MM-DD.
    pub date: String,
    pub date_label: String,
    pub assignee: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DedicatedSlot {
    pub id: String,
    pub label: String,
    pub candidates: Vec<Choice>,
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
    /// A printed card can be read from a photo to fill the form.
    pub can_scan: bool,
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
    /// The attributes as an editable form, for who manages the resource.
    pub can_manage: bool,
    pub fields: Vec<Field>,
    pub can_scan: bool,
    pub pictures: Vec<Picture>,
    pub places: Vec<Place>,
}

/// A stored picture: `reference` opens it through `attachment`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Picture {
    pub label: String,
    pub reference: String,
}

/// A point on the map, with a link that opens it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Place {
    pub label: String,
    pub lat: f64,
    pub lng: f64,
    pub link: String,
}

/// What a printed card said: values for the form (pictures as `data:`
/// URLs), whether text could be read on this device, and the fields left
/// empty.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ScanResult {
    pub values: JsonMap,
    pub text_read: bool,
    pub missing: Vec<String>,
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
    /// Qualifications held today, with their grant and start date.
    pub held: Vec<HeldQualification>,
    /// Roles held today.
    pub held_roles: Vec<HeldRole>,
    /// The units the person belongs to today.
    pub unit_ids: Vec<UnitRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HeldRole {
    pub role: String,
    pub grant_id: String,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct UnitRef {
    pub unit_id: String,
    pub unit_type: String,
    /// The membership, to end it.
    pub membership_id: String,
}

/// A qualification in the person form, with those it cannot be held with.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct QualificationOption {
    pub id: String,
    pub label: String,
    pub excludes: Vec<String>,
}

/// A role in the person form: a unit role is held in the person's own unit
/// of that type, unless it supervises others (`choose_unit`: pick the unit).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RoleOption {
    pub id: String,
    pub label: String,
    pub unit_type: Option<String>,
    pub choose_unit: bool,
    /// Units of the type, when the unit is chosen.
    pub units: Vec<Choice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HeldQualification {
    pub qualification: String,
    pub grant_id: String,
    /// YYYY-MM-DD.
    pub from: String,
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
    /// Qualifications edited as a date (`ui.people.dated_qualifications`).
    pub dated: Vec<Choice>,
    /// Qualifications edited as a box with a date (`ui.people.qualifications`).
    pub appointments: Vec<QualificationOption>,
    /// Attributes edited with the devices (`ui.people.with_devices`).
    pub with_devices: Vec<String>,
    pub role_options: Vec<RoleOption>,
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
    /// The organization's id (hex), for the blind node's configuration.
    pub org_id: Option<String>,
    /// Public key of the blind backup node (§10.3), if declared.
    pub backup_node: Option<String>,
    /// Root administrators set or remove it.
    pub can_set_backup_node: bool,
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
