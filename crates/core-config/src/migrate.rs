//! Checks between two versions of a package (PROGETTO.md §6.6): an element
//! that disappears must be declared in `migrations` as renamed (`rename`)
//! or retired (`retire`), otherwise the new version is refused.

use std::collections::{BTreeMap, BTreeSet};

use core_model::Slug;
use core_model::config::{Migration, PackageConfig};

use crate::Issue;

/// Elements identified by a slug that objects may refer to: (kind, file, id).
fn elements(pkg: &PackageConfig) -> BTreeSet<(&'static str, &'static str, Slug)> {
    let mut out = BTreeSet::new();
    for q in &pkg.qualifications.qualifications {
        out.insert(("qualification", "qualifications.yaml", q.id.clone()));
    }
    for u in &pkg.units.unit_types {
        out.insert(("unit type", "units.yaml", u.id.clone()));
    }
    for r in &pkg.roles.roles {
        out.insert(("role", "roles.yaml", r.id.clone()));
    }
    for a in &pkg.audiences.audiences {
        out.insert(("audience", "audiences.yaml", a.id.clone()));
    }
    for f in &pkg.forms.forms {
        out.insert(("form", "forms.yaml", f.id.clone()));
    }
    for w in &pkg.workflows.workflows {
        out.insert(("workflow", "workflows.yaml", w.id.clone()));
    }
    for a in &pkg.aggregates.aggregates {
        out.insert(("aggregate", "aggregates.yaml", a.id.clone()));
    }
    for e in &pkg.calendar.event_types {
        out.insert(("event type", "calendar.yaml", e.id.clone()));
    }
    for p in &pkg.programs.program_types {
        out.insert(("program type", "programs.yaml", p.id.clone()));
        for s in &p.slots {
            out.insert(("slot", "programs.yaml", s.id.clone()));
        }
    }
    for r in &pkg.resources.resource_types {
        out.insert(("resource type", "resources.yaml", r.id.clone()));
    }
    out
}

/// One element-level difference between two versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// `qualification`, `role`, `form`, `slot`, `texts`, …
    pub kind: String,
    pub id: String,
    /// `added`, `removed`, `renamed` (to `renamed_to`), `changed`.
    pub what: &'static str,
    pub renamed_to: Option<String>,
}

/// Each element as JSON, keyed by (kind, id), for comparisons.
fn element_values(pkg: &PackageConfig) -> BTreeMap<(String, String), serde_json::Value> {
    let mut out = BTreeMap::new();
    let mut put = |kind: &str, id: String, v: serde_json::Result<serde_json::Value>| {
        out.insert((kind.to_owned(), id), v.unwrap_or_default());
    };
    for q in &pkg.qualifications.qualifications {
        put("qualification", q.id.to_string(), serde_json::to_value(q));
    }
    for u in &pkg.units.unit_types {
        put("unit type", u.id.to_string(), serde_json::to_value(u));
    }
    for r in &pkg.roles.roles {
        put("role", r.id.to_string(), serde_json::to_value(r));
    }
    for a in &pkg.audiences.audiences {
        put("audience", a.id.to_string(), serde_json::to_value(a));
    }
    for f in &pkg.forms.forms {
        put("form", f.id.to_string(), serde_json::to_value(f));
    }
    for w in &pkg.workflows.workflows {
        put("workflow", w.id.to_string(), serde_json::to_value(w));
    }
    for a in &pkg.aggregates.aggregates {
        put("aggregate", a.id.to_string(), serde_json::to_value(a));
    }
    for e in &pkg.calendar.event_types {
        put("event type", e.id.to_string(), serde_json::to_value(e));
    }
    for p in &pkg.programs.program_types {
        let mut header = serde_json::to_value(p).unwrap_or_default();
        if let Some(o) = header.as_object_mut() {
            o.remove("slots");
        }
        put("program type", p.id.to_string(), Ok(header));
        for s in &p.slots {
            put(
                "slot",
                format!("{}.{}", p.id, s.id),
                serde_json::to_value(s),
            );
        }
    }
    for r in &pkg.resources.resource_types {
        put("resource type", r.id.to_string(), serde_json::to_value(r));
    }
    for (i, n) in pkg.notifications.notifications.iter().enumerate() {
        put("notification", i.to_string(), serde_json::to_value(n));
    }
    for (i, r) in pkg.notifications.reminders.iter().enumerate() {
        put("reminder", i.to_string(), serde_json::to_value(r));
    }
    for (locale, texts) in &pkg.texts {
        for (k, v) in texts {
            put(
                &format!("text {locale}"),
                k.clone(),
                Ok(serde_json::Value::String(v.clone())),
            );
        }
    }
    out
}

/// What changes from `old` to `new`, element by element (texts by key).
pub fn diff(old: &PackageConfig, new: &PackageConfig) -> Vec<Change> {
    let a = element_values(old);
    let b = element_values(new);
    let renamed = renames(new);
    let mut out = Vec::new();
    for ((kind, id), v) in &a {
        match b.get(&(kind.clone(), id.clone())) {
            Some(w) if w == v => {}
            Some(_) => out.push(Change {
                kind: kind.clone(),
                id: id.clone(),
                what: "changed",
                renamed_to: None,
            }),
            None => {
                let to = Slug::new(id)
                    .ok()
                    .and_then(|s| renamed.get(&s).map(|t| t.to_string()));
                let what = if to.is_some() { "renamed" } else { "removed" };
                out.push(Change {
                    kind: kind.clone(),
                    id: id.clone(),
                    what,
                    renamed_to: to,
                });
            }
        }
    }
    let renamed_to: BTreeSet<String> = renamed.values().map(|s| s.to_string()).collect();
    for (kind, id) in b.keys() {
        if !a.contains_key(&(kind.clone(), id.clone())) && !renamed_to.contains(id) {
            out.push(Change {
                kind: kind.clone(),
                id: id.clone(),
                what: "added",
                renamed_to: None,
            });
        }
    }
    out
}

/// Renames declared by a version: old id → new id.
pub fn renames(pkg: &PackageConfig) -> BTreeMap<Slug, Slug> {
    pkg.manifest
        .migrations
        .iter()
        .filter_map(|m| match m {
            Migration::Rename { from, to } => Some((from.clone(), to.clone())),
            Migration::Retire { .. } => None,
        })
        .collect()
}

/// Problems of `new` as the next version after `old`.
pub fn check_migrations(old: &PackageConfig, new: &PackageConfig) -> Vec<Issue> {
    let before = elements(old);
    let after = elements(new);
    let ids_after: BTreeSet<(&str, &Slug)> = after.iter().map(|(k, _, id)| (*k, id)).collect();
    let renamed = renames(new);
    let retired: BTreeSet<&Slug> = new
        .manifest
        .migrations
        .iter()
        .filter_map(|m| match m {
            Migration::Retire { id, .. } => Some(id),
            Migration::Rename { .. } => None,
        })
        .collect();
    let mut issues = Vec::new();
    for (kind, file, id) in &before {
        if ids_after.contains(&(*kind, id)) {
            continue;
        }
        match renamed.get(id) {
            Some(to) if ids_after.contains(&(*kind, to)) => {}
            Some(to) => issues.push(Issue::error(
                "package.yaml",
                "migrations",
                format!("{kind} `{id}` is renamed to `{to}`, which does not exist"),
            )),
            None if retired.contains(id) => {}
            None => issues.push(Issue::error(
                *file,
                "",
                format!(
                    "{kind} `{id}` was removed: declare it in `migrations` of package.yaml \
                     (`rename: {{from, to}}` or `retire: {{id}}`)"
                ),
            )),
        }
    }
    for (from, to) in &renamed {
        if after.iter().any(|(_, _, id)| id == from) {
            issues.push(Issue::error(
                "package.yaml",
                "migrations",
                format!("`{from}` is renamed to `{to}` but still exists"),
            ));
        }
    }
    issues
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::Path;

    fn club() -> PackageConfig {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/club");
        crate::load_and_validate(&dir).unwrap().0
    }

    fn slug(s: &str) -> Slug {
        Slug::new(s).unwrap()
    }

    #[test]
    fn removed_elements_need_a_migration() {
        let old = club();
        assert!(
            check_migrations(&old, &old).is_empty(),
            "same version is fine"
        );

        let mut new = old.clone();
        new.roles.roles.retain(|r| r.id.as_str() != "keeper");
        let issues = check_migrations(&old, &new);
        assert_eq!(issues.len(), 1);
        assert!(
            issues[0].message.contains("role `keeper` was removed"),
            "{issues:?}"
        );

        new.manifest.migrations.push(Migration::Retire {
            id: slug("keeper"),
            until_closed: true,
        });
        assert!(check_migrations(&old, &new).is_empty());
    }

    #[test]
    fn diff_lists_changes() {
        let old = club();
        let mut new = old.clone();
        new.programs.program_types[0].slots[0].count += 1;
        new.texts
            .values_mut()
            .next()
            .unwrap()
            .insert("x.new".into(), "New".into());
        let d = diff(&old, &new);
        assert!(
            d.iter().any(|c| c.kind == "slot" && c.what == "changed"),
            "{d:?}"
        );
        assert!(
            d.iter()
                .any(|c| c.kind.starts_with("text") && c.id == "x.new" && c.what == "added")
        );
        assert!(diff(&old, &old).is_empty());
    }

    #[test]
    fn renames_must_point_to_an_existing_element() {
        let old = club();
        let mut new = old.clone();
        let role = new
            .roles
            .roles
            .iter_mut()
            .find(|r| r.id.as_str() == "keeper")
            .unwrap();
        role.id = slug("archivist");
        new.manifest.migrations.push(Migration::Rename {
            from: slug("keeper"),
            to: slug("archivist"),
        });
        assert!(
            check_migrations(&old, &new).is_empty(),
            "{:?}",
            check_migrations(&old, &new)
        );
        assert_eq!(renames(&new).get(&slug("keeper")), Some(&slug("archivist")));

        let mut bad = new.clone();
        bad.manifest.migrations = vec![Migration::Rename {
            from: slug("keeper"),
            to: slug("nobody"),
        }];
        assert!(
            check_migrations(&old, &bad)
                .iter()
                .any(|i| i.message.contains("does not exist"))
        );
    }
}
