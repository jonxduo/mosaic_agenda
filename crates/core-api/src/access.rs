//! Which views a person may open. A management view is shown, and served,
//! only to who can act in it: without the permission it would be a page of
//! disabled controls (DECISIONS.md, "Schermate secondo i permessi").

use core_engine::time::date_of;
use core_model::config::{ViewKind, ViewParams};
use core_model::strings::{ObjectKind, ObjectRef};
use core_model::{Permission, PersonId};

use crate::views::cfg;
use crate::{ApiError, ApiResult, Core, View};

/// The permissions that make a view useful, any one being enough; empty
/// when everybody may open it (what it shows is filtered by other rules).
pub(crate) fn view_permissions(
    kind: ViewKind,
    params: &ViewParams,
) -> Vec<(Permission, Option<ObjectRef>)> {
    use Permission as P;
    match kind {
        ViewKind::QualificationMatrix => vec![(P::QualificationsManage, None)],
        ViewKind::People => vec![
            (P::PeopleManage, None),
            (P::QualificationsManage, None),
            (P::RolesGrant, None),
        ],
        ViewKind::Units => vec![(P::UnitsManage, None)],
        ViewKind::Roles => vec![(P::RolesGrant, None)],
        ViewKind::ConfigEditor => vec![(P::ConfigPublish, None)],
        ViewKind::ResourceList => {
            let obj = params.resource_type.as_ref().map(|rt| ObjectRef {
                kind: ObjectKind::Resource,
                id: rt.clone(),
            });
            vec![(P::ResourceManage, obj.clone()), (P::ResourceAssign, obj)]
        }
        _ => Vec::new(),
    }
}

/// Whether `person` holds any of `needed` today (root holds everything).
pub(crate) fn holds_any(
    v: &View,
    person: PersonId,
    needed: &[(Permission, Option<ObjectRef>)],
    now_ms: i64,
) -> bool {
    if needed.is_empty() || v.proj.auth.is_root(person) {
        return true;
    }
    let Ok(c) = cfg(v) else { return false };
    let date = date_of(now_ms);
    needed.iter().any(|(p, obj)| {
        v.proj
            .auth
            .is_allowed(c, person, *p, obj.as_ref(), None, date)
    })
}

impl Core {
    /// Refuses a view this user may not open: hiding it from the menu is
    /// not enough, a stale link or another client could still ask for it.
    pub(crate) fn ensure_view(&self, kind: ViewKind, params: &ViewParams) -> ApiResult<()> {
        let v = self.view()?;
        let me = self.me()?;
        if holds_any(&v, me, &view_permissions(kind, params), self.now()) {
            Ok(())
        } else {
            Err(ApiError::new(
                "denied",
                "this view needs a permission you do not hold",
            ))
        }
    }
}
