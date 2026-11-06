//! Configuration editor (PROGETTO.md §6.6, §12.4): the files of the active
//! version, validation (with migrations), simulation on a copy of the
//! projection, differences and publication of a new version.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use core_engine::Engine;
use core_model::eval::{EvalContext, Evaluator};
use core_model::{Permission, PersonId};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::views::{cfg, name_of, today};
use crate::{ApiError, ApiResult, Core, View};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ConfigFile {
    /// Path in the package (`roles.yaml`, `texts/it.yaml`, …).
    pub path: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ConfigIssue {
    /// `error` or `warning`.
    pub severity: String,
    pub file: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ConfigVersion {
    /// Active version of the organization.
    pub version: u32,
    /// Version of the package inside it (`package.yaml`).
    pub package_version: String,
    pub files: Vec<ConfigFile>,
    /// This person may publish a new version.
    pub can_publish: bool,
    /// The app carries a newer version of the package than the active one.
    pub app_update: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ConfigChange {
    pub kind: String,
    pub id: String,
    /// `added`, `removed`, `renamed`, `changed`.
    pub what: String,
    pub renamed_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AudienceChange {
    pub audience: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ConfigSimulation {
    pub issues: Vec<ConfigIssue>,
    pub changes: Vec<ConfigChange>,
    /// Who enters or leaves each audience today.
    pub audiences: Vec<AudienceChange>,
    /// Alerts that appear / disappear (e.g. role grants no longer valid).
    pub new_alerts: Vec<String>,
    pub gone_alerts: Vec<String>,
    /// Open documents: they keep the version they were created with.
    pub open_documents: u32,
}

fn to_map(files: &[ConfigFile]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|f| (f.path.clone(), f.text.clone()))
        .collect()
}

fn issue(i: &core_config::Issue) -> ConfigIssue {
    ConfigIssue {
        severity: match i.severity {
            core_config::Severity::Error => "error".into(),
            core_config::Severity::Warning => "warning".into(),
        },
        file: i.file.clone(),
        path: i.path.clone(),
        message: i.message.clone(),
    }
}

impl Core {
    pub(crate) fn may_publish(&self, v: &View) -> ApiResult<bool> {
        let me = self.me()?;
        let c = cfg(v)?;
        Ok(v.proj.auth.is_root(me)
            || v.proj
                .auth
                .is_allowed(c, me, Permission::ConfigPublish, None, None, today(self)))
    }

    /// The files of the active configuration version.
    pub fn config_files(&self) -> ApiResult<ConfigVersion> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let c = cfg(&v)?;
        let files = c
            .compiled
            .sources
            .iter()
            .map(|(path, text)| ConfigFile {
                path: path.clone(),
                text: text.clone(),
            })
            .collect();
        let active = c.compiled.package.manifest.version.clone();
        let app = self.package.package.manifest.version.clone();
        let newer = core_config::version_matches(&format!(">{active}"), &app).unwrap_or(false);
        Ok(ConfigVersion {
            version: v.proj.active_config,
            package_version: active,
            files,
            can_publish: self.may_publish(&v)?,
            app_update: newer.then_some(app),
        })
    }

    /// The files of the package carried by the app (to review and publish
    /// when it is newer than the active one).
    pub fn config_app_files(&self) -> ApiResult<Vec<ConfigFile>> {
        self.ensure_unlocked()?;
        Ok(self
            .package
            .sources
            .iter()
            .map(|(path, text)| ConfigFile {
                path: path.clone(),
                text: text.clone(),
            })
            .collect())
    }

    /// Validation of edited files, migrations against the active version included.
    pub fn config_check(&self, files: Vec<ConfigFile>) -> ApiResult<Vec<ConfigIssue>> {
        Ok(self.load_checked(&files)?.1)
    }

    fn load_checked(
        &self,
        files: &[ConfigFile],
    ) -> ApiResult<(Option<core_model::config::PackageConfig>, Vec<ConfigIssue>)> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let active = &cfg(&v)?.compiled.package;
        let pkg = match core_config::load_files(&to_map(files)) {
            Ok(p) => p,
            Err(issues) => return Ok((None, issues.iter().map(issue).collect())),
        };
        let mut issues: Vec<core_config::Issue> = core_config::validate(&pkg, None);
        issues.extend(core_config::check_migrations(active, &pkg));
        if pkg.manifest.id != active.manifest.id {
            issues.push(core_config::Issue::error(
                "package.yaml",
                "id",
                format!("the organization uses package `{}`", active.manifest.id),
            ));
        }
        let ok = !issues
            .iter()
            .any(|i| i.severity == core_config::Severity::Error);
        Ok((ok.then_some(pkg), issues.iter().map(issue).collect()))
    }

    /// Applies the edited files to a copy of the projection and reports what
    /// would change, without publishing anything.
    pub fn config_simulate(&self, files: Vec<ConfigFile>) -> ApiResult<ConfigSimulation> {
        let (pkg, issues) = self.load_checked(&files)?;
        let v = self.view()?;
        let empty = ConfigSimulation {
            issues: issues.clone(),
            changes: Vec::new(),
            audiences: Vec::new(),
            new_alerts: Vec::new(),
            gone_alerts: Vec::new(),
            open_documents: 0,
        };
        let Some(pkg) = pkg else { return Ok(empty) };
        let active = cfg(&v)?.compiled.package.clone();
        let changes = core_config::diff(&active, &pkg)
            .into_iter()
            .map(|c| ConfigChange {
                kind: c.kind,
                id: c.id,
                what: c.what.into(),
                renamed_to: c.renamed_to,
            })
            .collect();
        let bundle = core_config::compile_with_sources(pkg, to_map(&files))
            .map_err(ApiError::internal)?
            .bytes;
        let next = v.proj.configs.versions.keys().max().copied().unwrap_or(1) + 1;
        // The events a publication would create, on a copy of the log.
        let client = core_engine::actions::Client {
            events: &v.events,
            proj: &v.proj,
            keys: &self.keys,
            keyring: &v.ring,
            now: self.now(),
            engine: Some(v.engine.as_ref()),
        };
        let new_events = client
            .publish_config(next, bundle)
            .map_err(ApiError::from)?;
        let mut all = v.events.clone();
        all.extend(new_events);
        let engine = Engine::build(all, v.ring.clone()).map_err(ApiError::internal)?;
        let after = View {
            events: engine.events().to_vec(),
            proj: engine.projection().clone(),
            ring: engine.keyring().clone(),
            engine: Arc::new(Mutex::new(engine)),
        };
        let before_alerts: BTreeSet<String> =
            self.alerts_of(&v)?.into_iter().map(|a| a.message).collect();
        let after_alerts: BTreeSet<String> = self
            .alerts_of(&after)?
            .into_iter()
            .map(|a| a.message)
            .collect();
        let open_documents = v
            .proj
            .documents
            .values()
            .filter(|d| {
                v.proj.configs.get(d.content.form_version).is_some_and(|c| {
                    c.compiled
                        .package
                        .workflows
                        .workflows
                        .iter()
                        .flat_map(|w| w.steps.iter())
                        .any(|s| s.from.contains(&d.state))
                })
            })
            .count() as u32;
        Ok(ConfigSimulation {
            issues,
            changes,
            audiences: self.audience_changes(&v, &after)?,
            new_alerts: after_alerts.difference(&before_alerts).cloned().collect(),
            gone_alerts: before_alerts.difference(&after_alerts).cloned().collect(),
            open_documents,
        })
    }

    fn audience_changes(&self, before: &View, after: &View) -> ApiResult<Vec<AudienceChange>> {
        let ctx = EvalContext::at(today(self));
        let members = |v: &View, id: &core_model::Slug| -> BTreeSet<PersonId> {
            let Ok(c) = cfg(v) else {
                return BTreeSet::new();
            };
            Evaluator::new(c.index(), &v.proj.auth.org)
                .resolve(id, &ctx)
                .unwrap_or_default()
        };
        let ids: BTreeSet<core_model::Slug> = [before, after]
            .iter()
            .filter_map(|v| cfg(v).ok())
            .flat_map(|c| {
                c.compiled
                    .package
                    .audiences
                    .audiences
                    .iter()
                    .map(|a| a.id.clone())
            })
            .collect();
        let mut out = Vec::new();
        for id in ids {
            let a = members(before, &id);
            let b = members(after, &id);
            if a == b {
                continue;
            }
            out.push(AudienceChange {
                audience: id.to_string(),
                added: b.difference(&a).map(|p| name_of(after, *p)).collect(),
                removed: a.difference(&b).map(|p| name_of(before, *p)).collect(),
            });
        }
        Ok(out)
    }

    /// Publishes the edited files as the next configuration version.
    pub fn config_publish(self: &Arc<Self>, files: Vec<ConfigFile>) -> ApiResult<u32> {
        let (pkg, issues) = self.load_checked(&files)?;
        let Some(pkg) = pkg else {
            let first = issues
                .iter()
                .find(|i| i.severity == "error")
                .map(|i| format!("{}: {}", i.file, i.message))
                .unwrap_or_default();
            return Err(ApiError::invalid(first));
        };
        let v = self.view()?;
        if !self.may_publish(&v)? {
            return Err(ApiError::new(
                "denied",
                "not allowed to publish the configuration",
            ));
        }
        let next = v.proj.configs.versions.keys().max().copied().unwrap_or(1) + 1;
        let bundle = core_config::compile_with_sources(pkg, to_map(&files))
            .map_err(ApiError::internal)?
            .bytes;
        self.act_events(|c| c.publish_config(next, bundle))?;
        Ok(next)
    }
}
