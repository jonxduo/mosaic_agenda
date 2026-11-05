//! Event kinds (PROGETTO.md §7.3).
//!
//! postcard encodes enum variants by position: new kinds are only ever
//! appended at the end, never inserted or reordered.

use serde::{Deserialize, Serialize};

macro_rules! kinds {
    ($($variant:ident => $name:literal, $public:literal;)*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum EventKind {
            $(#[serde(rename = $name)] $variant,)*
        }

        impl EventKind {
            pub const ALL: &'static [EventKind] = &[$(EventKind::$variant,)*];

            /// Dotted name, e.g. `document.step`.
            pub fn as_str(self) -> &'static str {
                match self { $(EventKind::$variant => $name,)* }
            }

            /// Kinds that travel in clear: only the organizational metadata
            /// needed to check devices and keys and to sync (§7.1). Every
            /// other kind must be encrypted for a channel.
            pub fn is_public(self) -> bool {
                match self { $(EventKind::$variant => $public,)* }
            }

            pub fn parse(s: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|k| k.as_str() == s)
            }
        }
    };
}

kinds! {
    OrgGenesis => "org.genesis", true;
    OrgSettingsChanged => "org.settings_changed", true;
    ConfigPublished => "config.published", true;
    DeviceInvited => "device.invited", true;
    DeviceAdded => "device.added", true;
    DeviceRevoked => "device.revoked", true;
    DeviceArchiveSet => "device.archive_set", true;
    PersonAdded => "person.added", false;
    PersonUpdated => "person.updated", false;
    PersonRemoved => "person.removed", false;
    ExternalPersonUpserted => "external_person.upserted", false;
    QualificationGranted => "qualification.granted", false;
    QualificationEnded => "qualification.ended", false;
    UnitCreated => "unit.created", false;
    UnitUpdated => "unit.updated", false;
    UnitClosed => "unit.closed", false;
    MembershipStarted => "membership.started", false;
    MembershipEnded => "membership.ended", false;
    RoleGranted => "role.granted", false;
    RoleEnded => "role.ended", false;
    ChannelKeysDistributed => "channel.keys_distributed", true;
    ChannelRotated => "channel.rotated", true;
    DocumentCreated => "document.created", false;
    DocumentUpdated => "document.updated", false;
    DocumentStep => "document.step", false;
    DocumentDistributed => "document.distributed", false;
    BatchForwarded => "batch.forwarded", false;
    BatchRegistered => "batch.registered", false;
    AggregateGenerated => "aggregate.generated", false;
    CalendarEventUpserted => "calendar.event_upserted", false;
    CalendarEventCancelled => "calendar.event_cancelled", false;
    ProgramDrafted => "program.drafted", false;
    ProgramUpdated => "program.updated", false;
    ProgramAssignmentsSent => "program.assignments_sent", false;
    AssignmentResponded => "assignment.responded", false;
    ProgramPublished => "program.published", false;
    ProgramRevised => "program.revised", false;
    ResourceUpserted => "resource.upserted", false;
    ResourceAssigned => "resource.assigned", false;
    ResourceLogged => "resource.logged", false;
    ResourceRequestOpened => "resource.request_opened", false;
    ResourceRequestAnswered => "resource.request_answered", false;
    ResourceReturned => "resource.returned", false;
    ResourceRetired => "resource.retired", false;
    BoardPosted => "board.posted", false;
    BoardWithdrawn => "board.withdrawn", false;
    PersonDataErasureRequested => "person.data_erasure_requested", true;
    RetentionApplied => "retention.applied", true;
    AdminQuarantineResolved => "admin.quarantine_resolved", true;
    ProgramDetailsSet => "program.details_set", false;
    ProgramPartsShown => "program.parts_shown", false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_parse_back() {
        let mut names: Vec<&str> = EventKind::ALL.iter().map(|k| k.as_str()).collect();
        for k in EventKind::ALL {
            assert_eq!(EventKind::parse(k.as_str()), Some(*k));
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), EventKind::ALL.len());
    }

    /// Guards the postcard positions: reordering kinds breaks stored events.
    #[test]
    fn positions_are_stable() {
        let pos = |k: EventKind| postcard::to_allocvec(&k).unwrap_or_default();
        assert_eq!(pos(EventKind::OrgGenesis), vec![0]);
        assert_eq!(pos(EventKind::ChannelKeysDistributed), vec![20]);
        assert_eq!(pos(EventKind::DocumentStep), vec![24]);
        assert_eq!(pos(EventKind::AdminQuarantineResolved), vec![48]);
    }
}
