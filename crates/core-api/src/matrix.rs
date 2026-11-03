//! Persons × qualifications (view `qualification_matrix`): who holds which
//! of the qualifications the package lists, with a cell to grant or end each.

use std::sync::Arc;

use core_model::config::ViewKind;
use core_model::{Permission, PersonId, Slug};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::dto::Choice;
use crate::views::{UnitsOf, cfg, today};
use crate::{ApiError, ApiResult, Core, parse_person};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct QualificationMatrix {
    pub columns: Vec<Choice>,
    pub rows: Vec<MatrixRow>,
    pub can_manage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MatrixRow {
    pub person_id: String,
    pub name: String,
    pub unit: Option<String>,
    /// One per column, in order.
    pub cells: Vec<MatrixCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MatrixCell {
    pub held: bool,
    /// Whether the person meets the qualification's requirements (and
    /// holds none it excludes): only then it can be granted.
    pub allowed: bool,
}

/// One cell changed in the matrix.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct QualificationChange {
    pub person: String,
    pub qualification: String,
    pub held: bool,
}

impl Core {
    /// `qualifications`: comma-separated ids (the view's parameter).
    pub fn qualification_matrix(&self, qualifications: String) -> ApiResult<QualificationMatrix> {
        self.ensure_unlocked()?;
        self.ensure_view(ViewKind::QualificationMatrix, &Default::default())?;
        let v = self.view()?;
        let me = self.me()?;
        let c = cfg(&v)?;
        let t = self.texts();
        let now = today(self);
        let auth = &v.proj.auth;
        let defs: Vec<_> = qualifications
            .split(',')
            .filter(|s| !s.is_empty())
            .filter_map(|id| Slug::new(id).ok())
            .filter_map(|id| c.qualification(&id))
            .collect();
        let holds = |p: PersonId, q: &Slug| {
            auth.org.qualification_grants.iter().any(|g| {
                g.person == p
                    && &g.qualification == q
                    && g.period.start <= now
                    && g.period.end.is_none_or(|e| e > now)
            })
        };
        let mut rows: Vec<MatrixRow> = auth
            .org
            .persons
            .iter()
            .filter(|(p, r)| r.active && !v.proj.erasure.persons.contains(p))
            .map(|(p, r)| MatrixRow {
                person_id: p.to_string(),
                name: r.display_name.clone(),
                unit: auth.units_of_name(*p, now),
                cells: defs
                    .iter()
                    .map(|q| MatrixCell {
                        held: holds(*p, &q.id),
                        allowed: auth.satisfies(c, *p, q.requires.as_ref(), now)
                            && !q.excludes.iter().any(|x| holds(*p, x)),
                    })
                    .collect(),
            })
            .collect();
        rows.sort_by_key(|r| r.name.to_lowercase());
        Ok(QualificationMatrix {
            columns: defs
                .iter()
                .map(|q| Choice {
                    id: q.id.to_string(),
                    label: t.get(q.label.as_str()),
                })
                .collect(),
            rows,
            can_manage: auth.is_root(me)
                || auth.is_allowed(c, me, Permission::QualificationsManage, None, None, now),
        })
    }

    /// Grants or ends qualifications, one event each, then the matrix again.
    pub fn set_qualifications(
        self: &Arc<Self>,
        qualifications: String,
        changes: Vec<QualificationChange>,
    ) -> ApiResult<QualificationMatrix> {
        for ch in changes {
            let p = parse_person(&ch.person)?;
            let q = Slug::new(&ch.qualification).map_err(ApiError::invalid)?;
            if ch.held {
                self.grant_qualification(ch.person.clone(), ch.qualification.clone(), None, None)?;
                continue;
            }
            let now = today(self);
            let grants: Vec<String> = {
                let v = self.view()?;
                let auth = &v.proj.auth;
                auth.qualification_ids
                    .iter()
                    .filter_map(|(gid, i)| auth.org.qualification_grants.get(*i).map(|g| (gid, g)))
                    .filter(|(_, g)| {
                        g.person == p
                            && g.qualification == q
                            && g.period.end.is_none_or(|e| e > now)
                    })
                    .map(|(gid, _)| gid.to_string())
                    .collect()
            };
            for g in grants {
                self.end_grant("qualification".into(), g, None)?;
            }
        }
        self.qualification_matrix(qualifications)
    }
}
