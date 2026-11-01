//! Closed set of elementary permissions (PROGETTO.md §5.5).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::strings::{ObjectKind, ObjectRef};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum Permission {
    #[serde(rename = "document.fill")]
    DocumentFill,
    #[serde(rename = "document.fill_on_behalf")]
    DocumentFillOnBehalf,
    #[serde(rename = "document.read")]
    DocumentRead,
    #[serde(rename = "document.transition")]
    DocumentTransition,
    #[serde(rename = "batch.collect")]
    BatchCollect,
    #[serde(rename = "batch.forward")]
    BatchForward,
    #[serde(rename = "batch.register")]
    BatchRegister,
    #[serde(rename = "aggregate.generate")]
    AggregateGenerate,
    #[serde(rename = "document.distribute")]
    DocumentDistribute,
    #[serde(rename = "program.draft")]
    ProgramDraft,
    #[serde(rename = "program.assign")]
    ProgramAssign,
    #[serde(rename = "program.publish")]
    ProgramPublish,
    #[serde(rename = "assignment.respond_on_behalf")]
    AssignmentRespondOnBehalf,
    #[serde(rename = "calendar.manage")]
    CalendarManage,
    #[serde(rename = "resource.manage")]
    ResourceManage,
    #[serde(rename = "resource.assign")]
    ResourceAssign,
    #[serde(rename = "resource.log")]
    ResourceLog,
    #[serde(rename = "resource.request")]
    ResourceRequest,
    #[serde(rename = "board.publish")]
    BoardPublish,
    #[serde(rename = "board.withdraw")]
    BoardWithdraw,
    #[serde(rename = "people.manage")]
    PeopleManage,
    #[serde(rename = "qualifications.manage")]
    QualificationsManage,
    #[serde(rename = "units.manage")]
    UnitsManage,
    #[serde(rename = "roles.grant")]
    RolesGrant,
    #[serde(rename = "devices.manage")]
    DevicesManage,
    #[serde(rename = "config.publish")]
    ConfigPublish,
    #[serde(rename = "archive.serve")]
    ArchiveServe,
}

impl Permission {
    /// Object kinds an `on:` reference may name for this permission
    /// (empty = the permission takes no object).
    pub fn allowed_objects(self) -> &'static [ObjectKind] {
        use ObjectKind::*;
        use Permission::*;
        match self {
            DocumentFill | DocumentFillOnBehalf | DocumentTransition | BatchCollect
            | BatchForward | BatchRegister => &[Form],
            DocumentRead | DocumentDistribute => &[Form, Aggregate],
            AggregateGenerate => &[Aggregate],
            ProgramDraft | ProgramAssign | ProgramPublish | AssignmentRespondOnBehalf => &[Program],
            ResourceManage | ResourceAssign | ResourceLog | ResourceRequest => &[Resource],
            CalendarManage | BoardPublish | BoardWithdraw | PeopleManage | QualificationsManage
            | UnitsManage | RolesGrant | DevicesManage | ConfigPublish | ArchiveServe => &[],
        }
    }
}

/// Where a granted permission is exercised.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PermissionTarget {
    /// The unit of the object's subject.
    SubjectUnit,
    /// The scope of the role grant (the unit for unit roles, everything for organization roles).
    OwnScope,
    /// The whole organization.
    Organization,
}

/// A permission conferred by a role, optionally restricted to one object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PermissionGrant {
    pub permission: Permission,
    #[serde(default)]
    pub on: Option<ObjectRef>,
    /// Defaults to `own_scope`.
    #[serde(default)]
    pub target: Option<PermissionTarget>,
}
