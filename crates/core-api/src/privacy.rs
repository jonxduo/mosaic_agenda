//! Rights of the persons (PROGETTO.md §14): export of a person's data,
//! erasure request, retention.
//!
//! The erasure itself happens in [`crate::Core::view`]: the engine names the
//! events to erase and the store keeps only their shells.

use std::sync::Arc;

use core_engine::ops::Assignee;
use core_model::{Permission, PersonId};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::dto::{BoardLine, PersonView};
use crate::views::{cfg, display_value, state_label, today};
use crate::{ApiError, ApiResult, Core, parse_id};

/// Everything this device holds in clear about a person.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PersonExport {
    pub generated_at: String,
    pub person: PersonView,
    pub documents: Vec<ExportedDocument>,
    pub assignments: Vec<ExportedAssignment>,
    pub resources: Vec<ExportedHolding>,
    /// Erasure already requested for this person.
    pub erasure_requested: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ExportedDocument {
    pub form: String,
    pub period: String,
    pub state: String,
    pub values: Vec<BoardLine>,
    pub history: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ExportedAssignment {
    pub program: String,
    pub date: String,
    pub slot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ExportedHolding {
    pub resource: String,
    pub since: String,
    pub until: Option<String>,
    pub logs: u32,
}

impl Core {
    /// Data of a person, for the person themselves or for who manages people.
    pub fn export_person(&self, person: String) -> ApiResult<PersonExport> {
        self.ensure_unlocked()?;
        let p = PersonId(parse_id(&person)?);
        let v = self.view()?;
        let me = self.me()?;
        let now = today(self);
        let auth = &v.proj.auth;
        let may = me == p
            || auth.is_root(me)
            || auth.is_allowed(cfg(&v)?, me, Permission::PeopleManage, None, None, now);
        if !may {
            return Err(ApiError::new("denied", "export of another person's data"));
        }
        let t = self.texts();
        let cfg = cfg(&v)?;
        let view = self
            .people()?
            .persons
            .into_iter()
            .find(|x| x.person_id == person)
            .ok_or_else(|| ApiError::not_found("person"))?;
        let documents = v
            .proj
            .documents
            .values()
            .filter(|d| d.content.subject == p)
            .map(|d| {
                let form = cfg.form(&d.content.form);
                let fields = form
                    .and_then(|f| f.variants.iter().find(|x| x.id == d.content.variant))
                    .map(|x| x.fields.as_slice())
                    .unwrap_or_default();
                let values = fields
                    .iter()
                    .filter_map(|f| {
                        let val = d.content.values.get(&f.id)?;
                        let label = f.label.as_ref().map_or_else(
                            || t.get(&core_model::config::default_label::field(f.id.as_str())),
                            |l| t.get(l.as_str()),
                        );
                        Some(BoardLine {
                            label,
                            value: display_value(val),
                        })
                    })
                    .collect();
                ExportedDocument {
                    form: form
                        .map_or_else(|| d.content.form.to_string(), |f| t.get(f.label.as_str())),
                    period: t.period(&d.content.period),
                    state: state_label(&t, &d.state),
                    values,
                    history: d
                        .history
                        .iter()
                        .map(|h| format!("{} · {}", t.day_month(h.date), state_label(&t, &h.to)))
                        .collect(),
                }
            })
            .collect();
        let mut assignments = Vec::new();
        for prog in v.proj.programs.values() {
            for (key, (who, _)) in &prog.assignments {
                if *who != Assignee::Person(p) {
                    continue;
                }
                let pt = cfg.program_type(&prog.program_type);
                assignments.push(ExportedAssignment {
                    program: pt.map_or_else(
                        || prog.program_type.to_string(),
                        |x| t.get(x.label.as_str()),
                    ),
                    date: prog
                        .occurrences
                        .get(usize::from(key.occurrence))
                        .map(|d| d.to_string())
                        .unwrap_or_default(),
                    slot: pt
                        .and_then(|x| x.slots.iter().find(|s| s.id == key.slot))
                        .map_or_else(|| key.slot.to_string(), |s| t.get(s.label.as_str())),
                });
            }
        }
        assignments.sort_by(|a, b| a.date.cmp(&b.date));
        let mut resources = Vec::new();
        for r in v.proj.resources.values() {
            let name = r
                .attributes
                .values()
                .next()
                .map(display_value)
                .unwrap_or_else(|| r.resource_type.to_string());
            for h in r.history.iter().chain(r.holding.iter()) {
                if h.holder == p {
                    resources.push(ExportedHolding {
                        resource: name.clone(),
                        since: h.since.to_string(),
                        until: h.until.map(|d| d.to_string()),
                        logs: h.logs.len() as u32,
                    });
                }
            }
        }
        Ok(PersonExport {
            generated_at: now.to_string(),
            person: view,
            documents,
            assignments,
            resources,
            erasure_requested: v.proj.erasure.persons.contains(&p),
        })
    }

    /// The export as an indented JSON file at `path` (desktop save dialog).
    pub fn export_person_to(&self, person: String, path: &str) -> ApiResult<()> {
        let data = self.export_person(person)?;
        let json = serde_json::to_vec_pretty(&data).map_err(ApiError::internal)?;
        std::fs::write(path, json).map_err(ApiError::internal)
    }

    /// The package's privacy notice in the default language, at any time.
    pub fn privacy_notice(&self) -> ApiResult<Option<String>> {
        let pkg = &self.package.package;
        Ok(pkg
            .privacy_notices
            .get(&pkg.manifest.default_locale)
            .cloned())
    }

    /// Asks every device to erase the person's data (root administrators).
    pub fn request_erasure(self: &Arc<Self>, person: String) -> ApiResult<()> {
        let p = PersonId(parse_id(&person)?);
        self.act_events(|c| c.request_erasure(p))
    }

    /// Root administrators apply the retention periods once a week (§14).
    pub fn apply_retention_if_due(self: &Arc<Self>) -> ApiResult<bool> {
        if self.is_locked() {
            return Ok(false);
        }
        let v = self.view()?;
        let me = self.me()?;
        if !v.proj.auth.is_root(me) {
            return Ok(false);
        }
        let pkg = cfg(&v)?.package();
        let configured = pkg.forms.forms.iter().any(|f| f.retention.is_some())
            || pkg
                .programs
                .program_types
                .iter()
                .any(|p| p.retention.is_some());
        let now = today(self);
        let due = v
            .proj
            .erasure
            .retention
            .is_none_or(|last| (now - last).whole_days() >= 7);
        if !configured || !due {
            return Ok(false);
        }
        self.act_events(|c| c.apply_retention(now))?;
        Ok(true)
    }
}
