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
    /// The person may change it themselves (their profile).
    #[serde(default)]
    pub self_edit: bool,
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
    /// A point on the map: `"lat,lng"` in decimal degrees.
    Geo,
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
    /// The person may grant it to themselves and correct its date (their
    /// profile), e.g. a date they know best.
    #[serde(default)]
    pub self_service: bool,
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
    /// Subjects expected to hand in the form for the period (in scope).
    Expected,
    /// Expected subjects without a document.
    Missing,
    /// A number set in the configuration (`value`).
    Constant,
    /// Sum of other measures of the same aggregate (`of`).
    Total,
}

impl MeasureOp {
    /// Computed once for the whole scope, not per document or group.
    pub fn is_scope_wide(self) -> bool {
        matches!(self, Self::Expected | Self::Missing | Self::Constant)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeasureDef {
    pub id: Slug,
    /// Defaults to `msr.<id>`.
    #[serde(default)]
    pub label: Option<TextKey>,
    pub op: MeasureOp,
    /// Required for `count_true`, `sum`, `avg`, `min` and `max`.
    #[serde(default)]
    pub field: Option<Slug>,
    #[serde(default)]
    pub filter: Option<Cond>,
    /// The number of a `constant` measure.
    #[serde(default)]
    pub value: Option<f64>,
    /// The measures a `total` adds up (defined before it).
    #[serde(default)]
    pub of: Vec<Slug>,
    /// Heading the measure is shown under.
    #[serde(default)]
    pub section: Option<TextKey>,
    /// Shown in bold.
    #[serde(default)]
    pub strong: bool,
    /// Shown as a cell of a table: its row and its column (both set).
    #[serde(default)]
    pub row: Option<TextKey>,
    #[serde(default)]
    pub column: Option<TextKey>,
    /// Shown as a box with a large number, below the tables.
    #[serde(default)]
    pub tile: bool,
    /// Shown apart, as a note.
    #[serde(default)]
    pub note: bool,
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
    /// Program types handed in together for publication: who publishes sees
    /// them as one program per period, and publishes them at once.
    #[serde(default)]
    pub publish_groups: Vec<PublishGroupDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublishGroupDef {
    pub id: Slug,
    pub label: TextKey,
    #[serde(default)]
    pub icon: Option<Slug>,
    /// At least two, with the same period; a type in one group only.
    pub program_types: Vec<Slug>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProgramPeriod {
    Week,
    Month,
}

/// How long the same persons keep a slot within a program: a new choice for
/// every event, or one choice for all the events of a week or of a month.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    #[default]
    Occurrence,
    Week,
    Month,
}

/// How the persons of a slot change (Calendario e turni).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlotMode {
    /// As the program's `rotation`.
    #[default]
    Program,
    EachEvent,
    Month,
    /// Not at the events: rows of date and person added in the program.
    Dedicated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProgramOptionDef {
    pub id: Slug,
    pub label: TextKey,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionDef {
    pub id: Slug,
    pub label: TextKey,
    /// `#rrggbb` background of the heading.
    #[serde(default)]
    pub color: Option<String>,
}

impl ProgramTypeDef {
    /// Enabled, and its option (if any) is on.
    pub fn slot_active(&self, slot: &SlotDef) -> bool {
        slot.enabled
            && slot
                .option
                .as_ref()
                .is_none_or(|o| self.options.iter().any(|x| &x.id == o && x.enabled))
    }
}

/// When the assignees hear of their assignments: as soon as the team sends
/// the program, or only once it is published on the board (the team's
/// "send" then hands the program to who publishes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotifyOn {
    #[default]
    Send,
    Publish,
}

/// A web page per event whose outline (headings and the text below them)
/// fills the details of the slots: titles, minutes… Only empty details are
/// filled, and everything stays editable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrefillDef {
    /// `https://` address with `{year}` and `{week}` (ISO week of the
    /// event), `{date}` (YYYY-MM-DD), `{month}`, `{day}`.
    pub url: String,
    /// Regular expression on the text of a link of that page: the first
    /// link that matches is opened and read instead.
    #[serde(default)]
    pub follow: Option<String>,
    /// Tried in order on every heading of the page; the first that matches
    /// takes it.
    pub rules: Vec<PrefillRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrefillRule {
    /// Regular expression on the enclosing (higher) heading.
    #[serde(default)]
    pub section: Option<String>,
    /// Regular expression on the heading.
    pub heading: String,
    /// Regular expression on the text below the heading.
    #[serde(default)]
    pub text: Option<String>,
    /// The slot it fills, the next free position for repeated slots; none to
    /// recognize a heading and leave it alone.
    #[serde(default)]
    pub slot: Option<Slug>,
    /// Extra field → template with the named groups of the expressions,
    /// e.g. `{title}`.
    #[serde(default)]
    pub set: BTreeMap<Slug, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProgramTypeDef {
    pub id: Slug,
    pub label: TextKey,
    /// Short name (e.g. two letters), added to a slot named as one of
    /// another program type, where the name alone is ambiguous.
    #[serde(default)]
    pub short: Option<TextKey>,
    /// Name of an icon of the app (e.g. `book`, `mic`), shown on its tiles.
    #[serde(default)]
    pub icon: Option<Slug>,
    /// `#rrggbb` of its headings where several programs are shown together.
    #[serde(default)]
    pub color: Option<String>,
    pub event_type: OneOrMany<Slug>,
    pub period: ProgramPeriod,
    /// Assignments shared by the events of the same week or month.
    #[serde(default)]
    pub rotation: Rotation,
    /// What a slot of this program requires when added from the editor
    /// (Calendario e turni), whatever its name.
    #[serde(default)]
    pub slot_requires: Option<Cond>,
    /// Slots go to units of this type instead of persons (any unit may
    /// take one; its members hear of it).
    #[serde(default)]
    pub assign_units: Option<Slug>,
    pub owner_role: Slug,
    #[serde(default)]
    pub assistant_roles: Vec<Slug>,
    /// Assignees confirm or decline.
    #[serde(default = "yes")]
    pub acceptance: bool,
    #[serde(default)]
    pub notify: NotifyOn,
    /// Where the details of the slots can be read from for each event.
    #[serde(default)]
    pub prefill: Option<PrefillDef>,
    /// Switches that the administrators turn on or off (e.g. a second room):
    /// a slot with `option` is active only while it is on.
    #[serde(default)]
    pub options: Vec<ProgramOptionDef>,
    /// Headings that group the slots, in the order of the slots.
    #[serde(default)]
    pub sections: Vec<SectionDef>,
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
    /// Also a unit of this type may take the slot (e.g. a group conducting).
    #[serde(default)]
    pub allow_units: Option<Slug>,
    #[serde(default)]
    pub extra_fields: Vec<FieldDef>,
    #[serde(default)]
    pub repeat: Option<Repeat>,
    #[serde(default)]
    pub pair_with: Option<Slug>,
    /// Disabled slots stay in the package but are not assigned.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// A new person at every event, whatever the program's `rotation`
    /// (same as `mode: each_event`).
    #[serde(default)]
    pub each_event: bool,
    /// How the persons of this slot change: with the program (default), at
    /// every event, every month, or on dedicated days set in the program.
    #[serde(default)]
    pub mode: SlotMode,
    /// Section of the program (see `ProgramTypeDef::sections`): a heading is
    /// shown where the section changes.
    #[serde(default)]
    pub section: Option<Slug>,
    /// Active only while this option of the program type is on.
    #[serde(default)]
    pub option: Option<Slug>,
    /// Shown in the row of that slot (same position), in the column of its
    /// `option` (e.g. a second room).
    #[serde(default)]
    pub beside: Option<Slug>,
    /// Shown in the row of that slot, under its person (e.g. a reader).
    #[serde(default)]
    pub with: Option<Slug>,
    /// Takes a person; when false the slot only holds details (e.g. a number).
    #[serde(default = "yes")]
    pub assignable: bool,
    /// Positions shown in a new program; the others can be added per event
    /// (and any shown one removed). None: all.
    #[serde(default)]
    pub default_shown: Option<u32>,
    /// Repeated positions are labelled 1, 2, 3…
    #[serde(default = "yes")]
    pub numbered: bool,
    /// The editor shows the slot's name on the left of its row (off when
    /// the section and the details say it all).
    #[serde(default = "yes")]
    pub row_name: bool,
    /// Word shown before the person's name when the slot sits under another
    /// in the same row (e.g. "con", "Lettore").
    #[serde(default)]
    pub prefix: Option<TextKey>,
}

impl SlotDef {
    /// Whether a position is shown in a program unless the editor says otherwise.
    pub fn shown_by_default(&self, index: u32) -> bool {
        self.default_shown.is_none_or(|n| index < n)
    }
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
    /// Printed cards of this type can be read from a photo.
    #[serde(default)]
    pub scan: Option<ScanDef>,
}

/// The layout of a printed card (DECISIONS.md, "Lettura di schede da
/// foto"): every card is alike, so once straightened to `width` × `height`
/// each field sits in a known box.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScanDef {
    pub width: u32,
    pub height: u32,
    /// Text recognition language (Tesseract code, e.g. `ita`).
    #[serde(default)]
    pub language: Option<String>,
    pub boxes: Vec<ScanBox>,
}

/// One field of the card: `at` is `[x, y, width, height]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScanBox {
    pub field: Slug,
    pub at: [u32; 4],
    #[serde(default)]
    pub read: ScanRead,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScanRead {
    /// One line of text, often on a dotted leader.
    #[default]
    Line,
    /// One line of digits.
    Digits,
    /// Several lines of text.
    Text,
    /// A QR code; a map link in it fills a `geo` field.
    Code,
    /// The box itself, as a picture for a `photos` field.
    Image,
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
    /// What the people list shows on each person's row.
    #[serde(default)]
    pub people: PeopleUi,
    /// The personal area where everyone edits their own data.
    #[serde(default)]
    pub profile: ProfileUi,
}

/// The summary on a person's row: attributes, qualifications shown with
/// the date they were granted, and qualifications shown as marks.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeopleUi {
    #[serde(default)]
    pub attributes: Vec<Slug>,
    #[serde(default)]
    pub dated_qualifications: Vec<Slug>,
    #[serde(default)]
    pub qualifications: Vec<Slug>,
    /// Attributes edited with the devices, apart from the person's data
    /// (e.g. whether they use the app), saved as soon as they change.
    #[serde(default)]
    pub with_devices: Vec<Slug>,
}

/// The profile: what a person changes about themselves. Attributes must be
/// `self_edit`, qualifications `self_service`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileUi {
    /// Attributes holding the given name and the family name: the person's
    /// name becomes "family given".
    #[serde(default)]
    pub given_name: Option<Slug>,
    #[serde(default)]
    pub family_name: Option<Slug>,
    /// The other attributes of the profile, in order.
    #[serde(default)]
    pub fields: Vec<Slug>,
    /// Qualifications edited as a date (e.g. baptism).
    #[serde(default)]
    pub dated_qualifications: Vec<Slug>,
    /// Attribute filled with address suggestions (OpenStreetMap).
    #[serde(default)]
    pub address: Option<Slug>,
    /// ISO country codes the address suggestions are limited to.
    #[serde(default)]
    pub countries: Vec<String>,
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
    /// Persons × qualifications, each cell granted or not.
    QualificationMatrix,
    /// The units of a type one oversees: their members and the documents
    /// of a workflow, month by month (`workflow`, `unit_type`).
    UnitReports,
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
    /// Columns of `qualification_matrix`.
    #[serde(default)]
    pub qualifications: Vec<Slug>,
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
