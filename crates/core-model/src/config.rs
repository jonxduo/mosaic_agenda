//! Source configuration types: one struct per package file (PROGETTO.md §5, §6).
//!
//! These types are the single source of truth for `schemas/package.schema.json`.
//! Rules kept for postcard compatibility: no `untagged`/internally tagged
//! enums, no `flatten`, no `skip_serializing_if`.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::audience::AudienceDef;
use crate::cond::{Cond, Expr};
use crate::permission::PermissionGrant;
use crate::serde_util::OneOrMany;
use crate::strings::{GroupBy, ScopeDef, Slug, TextKey, VisibilityBound};

fn yes() -> bool {
    true
}

fn one() -> u32 {
    1
}

fn default_priority() -> i32 {
    100
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// `package.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: Slug,
    /// Semver of the source package.
    pub version: String,
    /// Compatible core versions, e.g. `">=1.0, <2.0"`.
    pub core_compat: String,
    pub default_locale: Slug,
    pub locales: Vec<Slug>,
    pub branding: Branding,
    /// Files of the package; all of them must exist (they may be empty).
    pub files: Vec<String>,
    #[serde(default)]
    pub person_attributes: Vec<PersonAttributeDef>,
    /// Path of the privacy notice, relative to the package (`<locale>` variants share the folder).
    pub privacy_notice: String,
    /// Terms that must never appear in the core (used by `check-domain-leak`).
    #[serde(default)]
    pub domain_terms: Vec<String>,
    /// Renames and retirements against the previous published version (§6.6).
    #[serde(default)]
    pub migrations: Vec<Migration>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Branding {
    pub app_name: TextKey,
    /// Reverse-DNS application identifier.
    pub identifier: String,
    /// 1024x1024 PNG, relative to the package.
    pub icon: String,
    /// `#RRGGBB`.
    pub primary_color: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PersonAttributeDef {
    pub id: Slug,
    #[serde(rename = "type")]
    pub ty: FieldType,
    /// Defaults to `fld.<id>`.
    #[serde(default)]
    pub label: Option<TextKey>,
    #[serde(default)]
    pub options: Option<Vec<ChoiceOption>>,
    #[serde(default)]
    pub max_length: Option<u32>,
    /// Who can read this attribute (it is encrypted for this audience).
    pub read_audience: Slug,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Migration {
    Rename {
        from: Slug,
        to: Slug,
    },
    Retire {
        id: Slug,
        #[serde(default = "yes")]
        until_closed: bool,
    },
}

// ---------------------------------------------------------------------------
// Fields
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Integer,
    Decimal,
    /// Minutes, shown as hours and minutes.
    Duration,
    Boolean,
    Text,
    LongText,
    Date,
    Period,
    Choice,
    MultiChoice,
    Person,
    Resource,
    Photos,
    Computed,
}

/// A choice option. In YAML either `id` or `{ id, label }`; label defaults to `opt.<id>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChoiceOption {
    pub id: Slug,
    pub label: Option<TextKey>,
}

impl JsonSchema for ChoiceOption {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ChoiceOption".into()
    }
    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let full = generator.subschema_for::<ChoiceOptionFull>();
        let short = generator.subschema_for::<Slug>();
        schemars::json_schema!({ "anyOf": [short, full] })
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ChoiceOptionFull {
    id: Slug,
    #[serde(default)]
    label: Option<TextKey>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ChoiceOptionRepr {
    Short(Slug),
    Full(ChoiceOptionFull),
}

impl<'de> Deserialize<'de> for ChoiceOption {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            Ok(match ChoiceOptionRepr::deserialize(d)? {
                ChoiceOptionRepr::Short(id) => Self { id, label: None },
                ChoiceOptionRepr::Full(f) => Self {
                    id: f.id,
                    label: f.label,
                },
            })
        } else {
            let f = ChoiceOptionFull::deserialize(d)?;
            Ok(Self {
                id: f.id,
                label: f.label,
            })
        }
    }
}

/// A field of a form variant, a slot, a resource or a log entry.
/// Options only apply to some types; `core-config` rejects misplaced ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldDef {
    pub id: Slug,
    #[serde(rename = "type")]
    pub ty: FieldType,
    /// Defaults to `fld.<id>`.
    #[serde(default)]
    pub label: Option<TextKey>,
    #[serde(default)]
    pub required: bool,
    /// integer, decimal, duration.
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub unit: Option<TextKey>,
    /// text, long_text.
    #[serde(default)]
    pub max_length: Option<u32>,
    /// choice, multi_choice.
    #[serde(default)]
    pub options: Option<Vec<ChoiceOption>>,
    /// person, resource.
    #[serde(default)]
    pub filter: Option<Cond>,
    /// photos.
    #[serde(default)]
    pub max_count: Option<u32>,
    /// photos: keep EXIF metadata (default: stripped).
    #[serde(default)]
    pub keep_exif: bool,
    /// computed.
    #[serde(default)]
    pub expr: Option<Expr>,
}

// ---------------------------------------------------------------------------
// Qualifications, units, roles, audiences
// ---------------------------------------------------------------------------

/// `qualifications.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualificationsFile {
    /// Named sets qualifications can belong to.
    #[serde(default)]
    pub sets: Vec<Slug>,
    #[serde(default)]
    pub qualifications: Vec<QualificationDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QualificationDuration {
    Permanent,
    FreePeriod,
    FixedPeriod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PeriodUnit {
    Week,
    Month,
    Year,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualificationDef {
    pub id: Slug,
    pub label: TextKey,
    pub duration: QualificationDuration,
    #[serde(default)]
    pub fixed_period_unit: Option<PeriodUnit>,
    #[serde(default)]
    pub implies: Vec<Slug>,
    #[serde(default)]
    pub excludes: Vec<Slug>,
    #[serde(default)]
    pub requires: Option<Cond>,
    #[serde(default)]
    pub sets: Vec<Slug>,
    /// Audience that sees that a person has this qualification.
    #[serde(default)]
    pub visibility: Option<Slug>,
}

/// `units.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnitsFile {
    #[serde(default)]
    pub unit_types: Vec<UnitTypeDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MembershipKind {
    /// A person belongs to at most one unit of this type at a time.
    Exclusive,
    Multiple,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnitTypeDef {
    pub id: Slug,
    pub label: TextKey,
    pub membership: MembershipKind,
    #[serde(default)]
    pub member_requires: Option<Cond>,
}

/// `roles.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RolesFile {
    #[serde(default)]
    pub roles: Vec<RoleDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Holders {
    #[serde(default)]
    pub min: u32,
    /// `null` or absent = unlimited.
    #[serde(default)]
    pub max: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoleDef {
    pub id: Slug,
    pub label: TextKey,
    pub scope: ScopeDef,
    #[serde(default)]
    pub requires: Option<Cond>,
    /// Holders per scope.
    #[serde(default)]
    pub holders: Holders,
    #[serde(default)]
    pub permissions: Vec<PermissionGrant>,
    /// Used to resolve concurrent workflow steps (higher wins).
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default)]
    pub substitute_for: Option<Slug>,
    #[serde(default)]
    pub exclusive_with: Vec<Slug>,
    #[serde(default)]
    pub supervises: Option<Slug>,
    #[serde(default)]
    pub assistant_of: Option<Slug>,
    /// When the scope must have a holder (or a substitute).
    #[serde(default)]
    pub mandatory: Option<Cond>,
}

/// `audiences.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudiencesFile {
    #[serde(default)]
    pub audiences: Vec<AudienceDef>,
}

// ---------------------------------------------------------------------------
// Forms, workflows, aggregates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Periodicity {
    None,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Offset {
    pub offset_days: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Days {
    pub days: u32,
}

/// How long closed items are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Retention {
    Days(u32),
    Months(u32),
    Years(u32),
    /// Kept without expiry (`forever: true`).
    Forever(bool),
}

/// `forms.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormsFile {
    #[serde(default)]
    pub forms: Vec<FormDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormDef {
    pub id: Slug,
    pub label: TextKey,
    /// Who must/can have this document.
    pub subject: Cond,
    pub periodicity: Periodicity,
    #[serde(default)]
    pub open_at: Option<Offset>,
    #[serde(default)]
    pub due: Option<Offset>,
    /// Evaluated in order; the first whose `when` holds wins.
    pub variants: Vec<VariantDef>,
    pub workflow: Slug,
    #[serde(default)]
    pub batching: Option<Batching>,
    /// Workflow state → audience reading the document in that state.
    #[serde(default)]
    pub read_audience_by_state: BTreeMap<Slug, Slug>,
    #[serde(default)]
    pub fillers_on_behalf: Option<Slug>,
    #[serde(default)]
    pub retention: Option<Retention>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VariantDef {
    pub id: Slug,
    pub when: Cond,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Batching {
    pub unit_type: Slug,
}

/// `workflows.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowsFile {
    #[serde(default)]
    pub workflows: Vec<WorkflowDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Highest role priority wins, then the lowest event id.
    #[default]
    PriorityThenHash,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDef {
    pub id: Slug,
    pub states: Vec<Slug>,
    pub initial: Slug,
    #[serde(default)]
    pub steps: Vec<StepDef>,
    #[serde(default)]
    pub timers: Vec<TimerDef>,
    #[serde(default)]
    pub conflict: ConflictPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Transition,
    /// Automatic: entering the state adds the document to its batch.
    Collect,
    Forward,
    Register,
    Aggregate,
    Distribute,
    Request,
}

impl StepKind {
    /// Steps performed automatically by the core, without an actor.
    pub fn is_automatic(self) -> bool {
        matches!(self, Self::Collect)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepRule {
    /// One member of the audience is enough.
    #[default]
    Any,
    /// Every member of the audience must perform it.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoteRequirement {
    #[default]
    None,
    Optional,
    Required,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepDef {
    pub id: Slug,
    /// Button label; defaults to `step.<id>`.
    #[serde(default)]
    pub label: Option<TextKey>,
    pub kind: StepKind,
    pub from: Vec<Slug>,
    pub to: Slug,
    /// Who can perform it (required except for automatic steps).
    #[serde(default)]
    pub by: Option<Slug>,
    #[serde(default)]
    pub rule: StepRule,
    #[serde(default)]
    pub when: Option<Cond>,
    #[serde(default)]
    pub allow_incomplete_after: Option<Offset>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub note: NoteRequirement,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotifySpec {
    pub audience: Slug,
    pub text: TextKey,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    SetReadAudience(Slug),
    Notify(NotifySpec),
    CreateAggregate(Slug),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimeAnchor {
    PeriodStart,
    PeriodEnd,
    Opened,
    Due,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimerAt {
    pub offset_days: i32,
    pub from: TimeAnchor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TimerAction {
    Remind(NotifySpec),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimerDef {
    pub at: TimerAt,
    #[serde(rename = "do")]
    pub action: TimerAction,
}

/// `aggregates.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AggregatesFile {
    #[serde(default)]
    pub aggregates: Vec<AggregateDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourcePeriod {
    #[default]
    Same,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AggregateSource {
    pub form: Slug,
    pub state: Slug,
    #[serde(default)]
    pub period: SourcePeriod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MeasureOp {
    Count,
    CountTrue,
    Sum,
    Avg,
    Min,
    Max,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeasureDef {
    pub id: Slug,
    /// Defaults to `msr.<id>`.
    #[serde(default)]
    pub label: Option<TextKey>,
    pub op: MeasureOp,
    /// Required for every op except `count`.
    #[serde(default)]
    pub field: Option<Slug>,
    #[serde(default)]
    pub filter: Option<Cond>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AggregateDef {
    pub id: Slug,
    pub label: TextKey,
    pub source: AggregateSource,
    #[serde(default)]
    pub group_by: Vec<GroupBy>,
    pub measures: Vec<MeasureDef>,
    #[serde(default)]
    pub include_individual_documents: bool,
    #[serde(default)]
    pub list_missing: bool,
    pub read_audience: Slug,
    #[serde(default)]
    pub retention: Option<Retention>,
}

// ---------------------------------------------------------------------------
// Calendar and programs
// ---------------------------------------------------------------------------

/// `calendar.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarFile {
    #[serde(default)]
    pub event_types: Vec<CalendarEventTypeDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CalendarEventTypeDef {
    pub id: Slug,
    pub label: TextKey,
    /// RFC 5545 RRULE, e.g. `FREQ=WEEKLY;BYDAY=TH`.
    #[serde(default)]
    pub default_recurrence: Option<String>,
    /// `HH:MM`.
    #[serde(default)]
    pub default_time: Option<String>,
    #[serde(default)]
    pub default_duration_min: Option<u32>,
    #[serde(default)]
    pub location_required: bool,
    #[serde(default)]
    pub scope: ScopeDef,
}

/// `programs.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProgramsFile {
    #[serde(default)]
    pub program_types: Vec<ProgramTypeDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProgramPeriod {
    Week,
    Month,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProgramTypeDef {
    pub id: Slug,
    pub label: TextKey,
    pub event_type: OneOrMany<Slug>,
    pub period: ProgramPeriod,
    pub owner_role: Slug,
    #[serde(default)]
    pub assistant_roles: Vec<Slug>,
    /// Assignees confirm or decline.
    #[serde(default = "yes")]
    pub acceptance: bool,
    pub slots: Vec<SlotDef>,
    pub publish_audience: Slug,
    #[serde(default)]
    pub remind_before: Option<Days>,
    #[serde(default)]
    pub retention: Option<Retention>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Repeat {
    /// Repeated as many times as the value of an extra field.
    Field(Slug),
    /// Repeated a fixed number of times.
    Count(u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SlotDef {
    pub id: Slug,
    pub label: TextKey,
    /// Persons per slot.
    #[serde(default = "one")]
    pub count: u32,
    #[serde(default)]
    pub requires: Option<Cond>,
    #[serde(default)]
    pub allow_external: bool,
    #[serde(default)]
    pub extra_fields: Vec<FieldDef>,
    #[serde(default)]
    pub repeat: Option<Repeat>,
    #[serde(default)]
    pub pair_with: Option<Slug>,
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

/// `resources.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourcesFile {
    #[serde(default)]
    pub resource_types: Vec<ResourceTypeDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestDef {
    pub id: Slug,
    pub label: TextKey,
    pub answers: Vec<ChoiceOption>,
    #[serde(default)]
    pub due: Option<Days>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceRules {
    #[serde(default)]
    pub max_per_holder: Option<u32>,
    #[serde(default)]
    pub remind_holder_after_no_log: Option<Days>,
    #[serde(default)]
    pub alert_manager_not_worked: Option<Days>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceReadAudience {
    pub list: Slug,
    pub history: Slug,
    /// The holder sees attachments while holding the resource.
    #[serde(default = "yes")]
    pub attachments_holder: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceTypeDef {
    pub id: Slug,
    pub label: TextKey,
    #[serde(default)]
    pub attributes: Vec<FieldDef>,
    /// Workflow id; default lifecycle: available → assigned → returned → available.
    #[serde(default)]
    pub lifecycle: Option<Slug>,
    #[serde(default)]
    pub holder_requires: Option<Cond>,
    pub manager_role: Slug,
    #[serde(default)]
    pub assistant_roles: Vec<Slug>,
    #[serde(default)]
    pub log_fields: Vec<FieldDef>,
    #[serde(default)]
    pub requests: Vec<RequestDef>,
    #[serde(default)]
    pub rules: ResourceRules,
    pub read_audience: ResourceReadAudience,
    #[serde(default)]
    pub retention: Option<Retention>,
}

// ---------------------------------------------------------------------------
// Board, notifications, UI
// ---------------------------------------------------------------------------

/// `board.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoardFile {
    #[serde(default)]
    pub board: Option<BoardDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoardDef {
    pub default_audience: Slug,
    pub default_visibility: Visibility,
    #[serde(default)]
    pub free_notices: FreeNotices,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Visibility {
    pub from: VisibilityBound,
    pub until: VisibilityBound,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FreeNotices {
    #[serde(default)]
    pub allowed: bool,
    #[serde(default)]
    pub audiences: Vec<Slug>,
}

/// `notifications.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotificationsFile {
    #[serde(default)]
    pub notifications: Vec<NotificationRuleDef>,
    #[serde(default)]
    pub reminders: Vec<ReminderRuleDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Trigger {
    #[serde(rename = "document.opened")]
    DocumentOpened,
    #[serde(rename = "document.state_changed")]
    DocumentStateChanged,
    #[serde(rename = "batch.forwarded")]
    BatchForwarded,
    #[serde(rename = "aggregate.distributed")]
    AggregateDistributed,
    #[serde(rename = "assignment.created")]
    AssignmentCreated,
    #[serde(rename = "assignment.declined")]
    AssignmentDeclined,
    #[serde(rename = "program.published")]
    ProgramPublished,
    #[serde(rename = "resource.assigned")]
    ResourceAssigned,
    #[serde(rename = "resource.request_opened")]
    ResourceRequestOpened,
    #[serde(rename = "qualification.expiring")]
    QualificationExpiring,
    #[serde(rename = "role.uncovered")]
    RoleUncovered,
    #[serde(rename = "grant.invalid")]
    GrantInvalid,
}

/// Optional restrictions on a trigger; every field that is set must match.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TriggerFilter {
    #[serde(default)]
    pub form: Option<Slug>,
    /// Target state for `document.state_changed`.
    #[serde(default)]
    pub to_state: Option<Slug>,
    #[serde(default)]
    pub aggregate: Option<Slug>,
    #[serde(default)]
    pub program_type: Option<Slug>,
    #[serde(default)]
    pub resource_type: Option<Slug>,
    #[serde(default)]
    pub qualification: Option<Slug>,
    /// For `qualification.expiring`.
    #[serde(default)]
    pub days_before: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotificationRuleDef {
    pub id: Slug,
    pub trigger: Trigger,
    #[serde(default)]
    pub filter: TriggerFilter,
    pub audience: Slug,
    pub text: TextKey,
    /// Category the user can mute (unless `mandatory`).
    #[serde(default)]
    pub category: Option<Slug>,
    #[serde(default)]
    pub mandatory: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ReminderKind {
    #[serde(rename = "document.missing_after")]
    DocumentMissingAfter,
    #[serde(rename = "batch.incomplete_after")]
    BatchIncompleteAfter,
    #[serde(rename = "batch.missing_after")]
    BatchMissingAfter,
    #[serde(rename = "event.assignment_before")]
    EventAssignmentBefore,
    #[serde(rename = "resource.no_log_after")]
    ResourceNoLogAfter,
    #[serde(rename = "request.due_after")]
    RequestDueAfter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReminderRuleDef {
    pub id: Slug,
    pub kind: ReminderKind,
    /// Days after (or before, for `event.assignment_before`) the reference moment.
    pub days: u32,
    #[serde(default)]
    pub filter: TriggerFilter,
    pub audience: Slug,
    pub text: TextKey,
    #[serde(default)]
    pub category: Option<Slug>,
    #[serde(default)]
    pub mandatory: bool,
}

/// `ui.yaml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiFile {
    #[serde(default)]
    pub home: Vec<ViewRef>,
    #[serde(default)]
    pub menu: Vec<MenuItem>,
}

/// Generic views of the core (PROGETTO.md §12.2) and home widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    Onboarding,
    Home,
    Form,
    MyDocuments,
    MyDocumentsDue,
    WorkQueue,
    Batch,
    Aggregate,
    ProgramList,
    ProgramEditor,
    MyAssignments,
    Board,
    ResourceList,
    ResourceDetail,
    MyResources,
    People,
    Units,
    Roles,
    SyncStatus,
    AdminAlerts,
    ConfigEditor,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewParams {
    #[serde(default)]
    pub workflow: Option<Slug>,
    #[serde(default)]
    pub form: Option<Slug>,
    #[serde(default)]
    pub aggregate: Option<Slug>,
    #[serde(default)]
    pub resource_type: Option<Slug>,
    #[serde(default)]
    pub unit_type: Option<Slug>,
    /// Program types for `program_list`.
    #[serde(default)]
    pub types: Vec<Slug>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ViewRef {
    pub view: ViewKind,
    #[serde(default)]
    pub params: ViewParams,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MenuItem {
    pub label: TextKey,
    pub view: ViewKind,
    #[serde(default)]
    pub params: ViewParams,
    #[serde(default)]
    pub visible_to: Option<Slug>,
}

// ---------------------------------------------------------------------------
// Whole package
// ---------------------------------------------------------------------------

/// Locale → (text key → text).
pub type Texts = BTreeMap<Slug, BTreeMap<String, String>>;

/// All the files of a package, parsed. Its JSON Schema (with one `$defs`
/// entry per file type) is `schemas/package.schema.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageConfig {
    pub manifest: Manifest,
    pub qualifications: QualificationsFile,
    pub units: UnitsFile,
    pub roles: RolesFile,
    pub audiences: AudiencesFile,
    pub forms: FormsFile,
    pub workflows: WorkflowsFile,
    pub aggregates: AggregatesFile,
    pub calendar: CalendarFile,
    pub programs: ProgramsFile,
    pub resources: ResourcesFile,
    pub board: BoardFile,
    pub notifications: NotificationsFile,
    pub ui: UiFile,
    pub texts: Texts,
    /// Locale → privacy notice (Markdown).
    pub privacy_notices: BTreeMap<Slug, String>,
}

/// Default label keys for elements whose `label` is optional.
pub mod default_label {
    pub fn field(id: &str) -> String {
        format!("fld.{id}")
    }
    pub fn option(id: &str) -> String {
        format!("opt.{id}")
    }
    pub fn measure(id: &str) -> String {
        format!("msr.{id}")
    }
    pub fn step(id: &str) -> String {
        format!("step.{id}")
    }
}
