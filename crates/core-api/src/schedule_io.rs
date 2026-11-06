//! Calendar and rotations as a file: `calendar.yaml`, `programs.yaml`, the
//! texts of their labels and the weekly series, exported in JSON or YAML and
//! imported into another organization (or back). Only who may publish the
//! configuration: the import publishes a new version.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::commands::slug;
use crate::config::ConfigFile;
use crate::views::cfg;
use crate::{ApiError, ApiResult, Core};

const KIND: &str = "calendar_and_rotations";
const CALENDAR: &str = "calendar.yaml";
const PROGRAMS: &str = "programs.yaml";

/// The exported file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleFile {
    pub kind: String,
    pub version: u32,
    pub calendar: String,
    pub programs: String,
    /// Texts (default locale) of the keys the two files use.
    #[serde(default)]
    pub texts: BTreeMap<String, String>,
    #[serde(default)]
    pub series: Vec<SeriesEntry>,
}

/// A weekly series; units by name, so that they match in another organization.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeriesEntry {
    pub event_type: String,
    /// 1 = Monday … 7 = Sunday.
    pub weekday: u8,
    pub time: String,
    pub location: String,
    #[serde(default)]
    pub units: Vec<String>,
    #[serde(default)]
    pub conducted_by_unit: bool,
}

/// Every string of a YAML document.
fn strings(v: &serde_norway::Value, out: &mut Vec<String>) {
    match v {
        serde_norway::Value::String(s) => out.push(s.clone()),
        serde_norway::Value::Sequence(items) => items.iter().for_each(|x| strings(x, out)),
        serde_norway::Value::Mapping(m) => m.values().for_each(|x| strings(x, out)),
        serde_norway::Value::Tagged(t) => strings(&t.value, out),
        _ => {}
    }
}

/// `text` with `key` set to `value`: a top-level `key:` line is replaced,
/// a missing key is added at the end. Nested keys are left as they are.
fn set_text(text: &str, key: &str, value: &str) -> String {
    let quoted = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned());
    let line = format!("{key}: {quoted}");
    let prefix = format!("{key}:");
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            if l.starts_with(&prefix) {
                found = true;
                line.clone()
            } else {
                l.to_owned()
            }
        })
        .collect();
    if !found {
        out.push(line);
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

impl Core {
    fn may_edit_schedule(&self) -> ApiResult<()> {
        let v = self.view()?;
        if self.may_publish(&v)? {
            Ok(())
        } else {
            Err(ApiError::new("denied", "not allowed to change the configuration"))
        }
    }

    /// Calendar and rotations as a file, `json` or `yaml`.
    pub fn schedule_export(&self, format: String) -> ApiResult<String> {
        self.ensure_unlocked()?;
        self.may_edit_schedule()?;
        let v = self.view()?;
        let c = cfg(&v)?;
        let source = |p: &str| c.compiled.sources.get(p).cloned().unwrap_or_default();
        let (calendar, programs) = (source(CALENDAR), source(PROGRAMS));
        let pkg = &c.compiled.package;
        let all = pkg.texts.get(&pkg.manifest.default_locale);
        let mut used = Vec::new();
        for text in [&calendar, &programs] {
            if let Ok(doc) = serde_norway::from_str::<serde_norway::Value>(text) {
                strings(&doc, &mut used);
            }
        }
        let texts = used
            .into_iter()
            .filter_map(|k| all.and_then(|a| a.get(&k)).map(|t| (k, t.clone())))
            .collect();
        let units = &v.proj.auth.org.units;
        let mut series: Vec<SeriesEntry> = v
            .proj
            .series
            .values()
            .map(|s| SeriesEntry {
                event_type: s.event_type.to_string(),
                weekday: s.weekday,
                time: s.time.clone(),
                location: s.location.clone(),
                units: s.units.iter().filter_map(|u| units.get(u).map(|r| r.name.clone())).collect(),
                conducted_by_unit: s.conducted_by_unit,
            })
            .collect();
        series.sort_by(|a, b| {
            (&a.event_type, a.weekday, &a.time, &a.location).cmp(&(&b.event_type, b.weekday, &b.time, &b.location))
        });
        let file = ScheduleFile { kind: KIND.to_owned(), version: 1, calendar, programs, texts, series };
        match format.as_str() {
            "json" => serde_json::to_string_pretty(&file).map_err(ApiError::internal),
            "yaml" => serde_norway::to_string(&file).map_err(ApiError::internal),
            other => Err(ApiError::invalid(format!("unknown format `{other}`"))),
        }
    }

    /// The export written to `path` (the app's save dialog).
    pub fn schedule_export_to(&self, format: String, path: &str) -> ApiResult<()> {
        let text = self.schedule_export(format)?;
        std::fs::write(path, text).map_err(ApiError::internal)
    }

    /// Imports a file of [`Core::schedule_export`] (JSON or YAML): publishes
    /// a new configuration version with its calendar, programs and texts,
    /// then puts its weekly series in place of the current ones. Returns the
    /// new version.
    pub fn schedule_import(self: &Arc<Self>, text: String) -> ApiResult<u32> {
        self.ensure_unlocked()?;
        self.may_edit_schedule()?;
        // JSON is YAML too.
        let file: ScheduleFile = serde_norway::from_str(&text)
            .map_err(|e| ApiError::invalid(format!("not a calendar file: {e}")))?;
        if file.kind != KIND {
            return Err(ApiError::invalid("not a calendar file"));
        }
        let (texts_path, mut files) = {
            let v = self.view()?;
            let c = cfg(&v)?;
            let locale = c.compiled.package.manifest.default_locale.to_string();
            let files: Vec<ConfigFile> = c
                .compiled
                .sources
                .iter()
                .map(|(path, text)| ConfigFile { path: path.clone(), text: text.clone() })
                .collect();
            (format!("texts/{locale}.yaml"), files)
        };
        for f in &mut files {
            if f.path == CALENDAR {
                f.text.clone_from(&file.calendar);
            } else if f.path == PROGRAMS {
                f.text.clone_from(&file.programs);
            } else if f.path == texts_path {
                for (k, t) in &file.texts {
                    f.text = set_text(&f.text, k, t);
                }
            }
        }
        let version = self.config_publish(files)?;

        // The series: the imported ones replace the current ones.
        let (old, new) = {
            let v = self.view()?;
            let by_name: BTreeMap<&str, core_model::UnitId> =
                v.proj.auth.org.units.iter().map(|(id, u)| (u.name.as_str(), *id)).collect();
            let old: Vec<core_model::Id> = v.proj.series.keys().copied().collect();
            let mut new = Vec::new();
            for s in &file.series {
                new.push(core_engine::ops::CalendarSeriesUpserted {
                    series: core_model::Id::random().map_err(ApiError::internal)?,
                    event_type: slug(&s.event_type)?,
                    weekday: s.weekday,
                    time: s.time.clone(),
                    location: s.location.clone(),
                    // Units of another organization that are not here are left out.
                    units: s.units.iter().filter_map(|n| by_name.get(n.as_str()).copied()).collect(),
                    conducted_by_unit: s.conducted_by_unit,
                });
            }
            (old, new)
        };
        for id in old {
            self.act_events(|c| c.remove_series(id))?;
        }
        for s in new {
            self.act_events(|c| c.upsert_series(s))?;
        }
        Ok(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_text_replaces_or_adds() {
        let t = "# texts\na.b: \"x\"\nc: d\n";
        assert_eq!(set_text(t, "a.b", "y"), "# texts\na.b: \"y\"\nc: d\n");
        assert_eq!(set_text(t, "e", "Z \"q\""), "# texts\na.b: \"x\"\nc: d\ne: \"Z \\\"q\\\"\"\n");
        // Another key with the same start is not touched.
        assert_eq!(set_text("ab: 1\n", "a", "2"), "ab: 1\na: \"2\"\n");
    }
}
