//! Reading a package directory into a typed [`PackageConfig`].

use std::collections::BTreeMap;
use std::path::Path;

use core_model::Slug;
use core_model::config::{Manifest, PackageConfig};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::issue::Issue;
use crate::schema::file_schema;

/// Standard files of a package and the schema definition of each.
pub const PACKAGE_FILES: &[(&str, &str)] = &[
    ("qualifications.yaml", "QualificationsFile"),
    ("units.yaml", "UnitsFile"),
    ("roles.yaml", "RolesFile"),
    ("audiences.yaml", "AudiencesFile"),
    ("forms.yaml", "FormsFile"),
    ("workflows.yaml", "WorkflowsFile"),
    ("aggregates.yaml", "AggregatesFile"),
    ("calendar.yaml", "CalendarFile"),
    ("programs.yaml", "ProgramsFile"),
    ("resources.yaml", "ResourcesFile"),
    ("board.yaml", "BoardFile"),
    ("notifications.yaml", "NotificationsFile"),
    ("ui.yaml", "UiFile"),
];

const MANIFEST: &str = "package.yaml";

/// Parses YAML text into JSON, expanding anchors and merge keys.
/// An empty document becomes `{}`.
pub(crate) fn yaml_to_json(file: &str, text: &str) -> Result<Value, Issue> {
    let mut yaml: serde_norway::Value = serde_norway::from_str(text)
        .map_err(|e| Issue::error(file, "", format!("invalid YAML: {e}")))?;
    yaml.apply_merge()
        .map_err(|e| Issue::error(file, "", format!("invalid YAML merge key: {e}")))?;
    let json = serde_json::to_value(&yaml)
        .map_err(|e| Issue::error(file, "", format!("unsupported YAML value: {e}")))?;
    Ok(match json {
        Value::Null => Value::Object(Default::default()),
        other => other,
    })
}

/// Drops top-level keys starting with `x-` (anchor holders).
fn strip_extensions(mut v: Value) -> Value {
    if let Value::Object(map) = &mut v {
        map.retain(|k, _| !k.starts_with("x-"));
    }
    v
}

/// `/roles/3/requires` → `roles[3].requires`.
pub(crate) fn pointer_to_path(pointer: &str) -> String {
    let mut out = String::new();
    for seg in pointer.split('/').skip(1) {
        let seg = seg.replace("~1", "/").replace("~0", "~");
        if seg.chars().all(|c| c.is_ascii_digit()) && !seg.is_empty() {
            out.push_str(&format!("[{seg}]"));
        } else {
            if !out.is_empty() {
                out.push('.');
            }
            out.push_str(&seg);
        }
    }
    out
}

/// Checks a value against the schema of one file definition.
fn schema_issues(file: &str, definition: &str, value: &Value) -> Vec<Issue> {
    let schema = file_schema(definition);
    let validator = match jsonschema::validator_for(&schema) {
        Ok(v) => v,
        Err(e) => {
            return vec![Issue::error(
                file,
                "",
                format!("internal schema error: {e}"),
            )];
        }
    };
    validator
        .iter_errors(value)
        .map(|e| {
            Issue::error(
                file,
                pointer_to_path(&e.instance_path().to_string()),
                e.to_string(),
            )
        })
        .collect()
}

/// Typed deserialization with the path of the failing element.
/// Serde's messages are more precise than schema ones, so they come first;
/// schema errors are reported only when serde accepts the value.
fn parse_typed<T: DeserializeOwned>(
    file: &str,
    definition: &str,
    value: Value,
) -> Result<T, Vec<Issue>> {
    let schema_errs = schema_issues(file, definition, &value);
    let typed: Result<T, _> = serde_path_to_error::deserialize(value);
    match typed {
        Err(e) => {
            let path = e.path().to_string();
            let path = if path == "." { String::new() } else { path };
            Err(vec![Issue::error(file, path, e.inner().to_string())])
        }
        Ok(_) if !schema_errs.is_empty() => Err(schema_errs),
        Ok(v) => Ok(v),
    }
}

/// Where the package files come from: a folder, or texts in memory (the
/// configuration editor).
#[derive(Clone, Copy)]
pub(crate) enum Source<'a> {
    Dir(&'a Path),
    Files(&'a BTreeMap<String, String>),
}

fn read(src: Source<'_>, rel: &str) -> Result<String, Issue> {
    match src {
        Source::Dir(dir) => std::fs::read_to_string(dir.join(rel))
            .map_err(|e| Issue::error(rel, "", format!("cannot read file: {e}"))),
        Source::Files(files) => files
            .get(rel)
            .cloned()
            .ok_or_else(|| Issue::error(rel, "", "missing file")),
    }
}

fn load_file<T: DeserializeOwned>(
    src: Source<'_>,
    rel: &str,
    definition: &str,
) -> Result<T, Vec<Issue>> {
    let text = read(src, rel).map_err(|e| vec![e])?;
    let json = yaml_to_json(rel, &text).map_err(|e| vec![e])?;
    parse_typed(rel, definition, strip_extensions(json))
}

/// Flattens nested maps into dotted keys; every leaf must be a string.
fn flatten_texts(
    file: &str,
    prefix: &str,
    v: &Value,
    out: &mut BTreeMap<String, String>,
    issues: &mut Vec<Issue>,
) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_texts(file, &key, child, out, issues);
            }
        }
        Value::String(s) => {
            if out.insert(prefix.to_owned(), s.clone()).is_some() {
                issues.push(Issue::error(file, prefix, "duplicate text key"));
            }
        }
        _ => issues.push(Issue::error(file, prefix, "text values must be strings")),
    }
}

/// Privacy notice path for a locale: the manifest path with its file stem
/// replaced by the locale (`privacy/it.md` → `privacy/en.md`).
fn privacy_path(manifest_path: &str, default_locale: &str, locale: &str) -> String {
    if locale == default_locale {
        return manifest_path.to_owned();
    }
    let p = Path::new(manifest_path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("md");
    match p.parent().and_then(|d| d.to_str()) {
        Some(d) if !d.is_empty() => format!("{d}/{locale}.{ext}"),
        _ => format!("{locale}.{ext}"),
    }
}

/// Loads every file of the package. Fails with all the errors found;
/// semantic checks are left to [`crate::validate`].
pub fn load_package(dir: &Path) -> Result<PackageConfig, Vec<Issue>> {
    load_from(Source::Dir(dir))
}

/// Like [`load_package`], from file texts keyed by their path in the package
/// (`package.yaml`, `roles.yaml`, `texts/it.yaml`, `privacy/it.md`, …).
pub fn load_files(files: &BTreeMap<String, String>) -> Result<PackageConfig, Vec<Issue>> {
    load_from(Source::Files(files))
}

/// The text files of a package folder, keyed by their path in the package:
/// what the configuration editor shows and edits.
pub fn source_files(dir: &Path) -> Result<BTreeMap<String, String>, Vec<Issue>> {
    let manifest: Manifest = load_file(Source::Dir(dir), MANIFEST, "Manifest")?;
    let mut paths: Vec<String> = vec![MANIFEST.to_owned()];
    paths.extend(manifest.files.iter().cloned());
    for locale in &manifest.locales {
        paths.push(format!("texts/{locale}.yaml"));
        paths.push(privacy_path(
            &manifest.privacy_notice,
            manifest.default_locale.as_str(),
            locale.as_str(),
        ));
    }
    let mut out = BTreeMap::new();
    let mut issues = Vec::new();
    for p in paths {
        match read(Source::Dir(dir), &p) {
            Ok(t) => {
                out.insert(p, t);
            }
            Err(e) => issues.push(e),
        }
    }
    if issues.is_empty() {
        Ok(out)
    } else {
        Err(issues)
    }
}

fn load_from(src: Source<'_>) -> Result<PackageConfig, Vec<Issue>> {
    let dir = src;
    let manifest: Manifest = load_file(dir, MANIFEST, "Manifest")?;
    let mut issues = Vec::new();

    for (i, f) in manifest.files.iter().enumerate() {
        if !PACKAGE_FILES.iter().any(|(name, _)| name == f) {
            issues.push(Issue::error(
                MANIFEST,
                format!("files[{i}]"),
                format!("unknown package file `{f}`"),
            ));
        }
    }
    for (name, _) in PACKAGE_FILES {
        if !manifest.files.iter().any(|f| f == name) {
            issues.push(Issue::error(
                MANIFEST,
                "files",
                format!("missing required file `{name}` (it may be empty)"),
            ));
        }
    }

    macro_rules! file {
        ($idx:expr) => {{
            let (name, def) = PACKAGE_FILES[$idx];
            match load_file(dir, name, def) {
                Ok(v) => Some(v),
                Err(mut e) => {
                    issues.append(&mut e);
                    None
                }
            }
        }};
    }
    let qualifications = file!(0);
    let units = file!(1);
    let roles = file!(2);
    let audiences = file!(3);
    let forms = file!(4);
    let workflows = file!(5);
    let aggregates = file!(6);
    let calendar = file!(7);
    let programs = file!(8);
    let resources = file!(9);
    let board = file!(10);
    let notifications = file!(11);
    let ui = file!(12);

    let mut texts: BTreeMap<Slug, BTreeMap<String, String>> = BTreeMap::new();
    let mut privacy_notices = BTreeMap::new();
    for locale in &manifest.locales {
        let rel = format!("texts/{locale}.yaml");
        match read(dir, &rel).and_then(|t| yaml_to_json(&rel, &t)) {
            Ok(json) => {
                let mut map = BTreeMap::new();
                flatten_texts(&rel, "", &json, &mut map, &mut issues);
                texts.insert(locale.clone(), map);
            }
            Err(e) => issues.push(e),
        }
        let rel = privacy_path(
            &manifest.privacy_notice,
            manifest.default_locale.as_str(),
            locale.as_str(),
        );
        match read(dir, &rel) {
            Ok(t) => {
                privacy_notices.insert(locale.clone(), t);
            }
            Err(e) if locale == &manifest.default_locale => issues.push(e),
            Err(_) => issues.push(Issue::warning(
                rel,
                "",
                format!("no privacy notice for locale `{locale}`"),
            )),
        }
    }

    if issues.iter().any(|i| i.severity == crate::Severity::Error) {
        return Err(issues);
    }
    match (
        qualifications,
        units,
        roles,
        audiences,
        forms,
        workflows,
        aggregates,
        calendar,
        programs,
        resources,
        board,
        notifications,
        ui,
    ) {
        (
            Some(qualifications),
            Some(units),
            Some(roles),
            Some(audiences),
            Some(forms),
            Some(workflows),
            Some(aggregates),
            Some(calendar),
            Some(programs),
            Some(resources),
            Some(board),
            Some(notifications),
            Some(ui),
        ) => Ok(PackageConfig {
            manifest,
            qualifications,
            units,
            roles,
            audiences,
            forms,
            workflows,
            aggregates,
            calendar,
            programs,
            resources,
            board,
            notifications,
            ui,
            texts,
            privacy_notices,
        }),
        _ => Err(issues),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_conversion() {
        assert_eq!(
            pointer_to_path("/roles/3/requires/all/0"),
            "roles[3].requires.all[0]"
        );
        assert_eq!(pointer_to_path(""), "");
        assert_eq!(pointer_to_path("/a~1b"), "a/b");
    }

    #[test]
    fn anchors_and_merge_keys_are_expanded() {
        let v = yaml_to_json(
            "x.yaml",
            "x-base: &b { a: 1 }\nitem: *b\nmerged:\n  <<: *b\n  c: 2\n",
        )
        .unwrap();
        let v = strip_extensions(v);
        assert_eq!(
            v,
            serde_json::json!({ "item": { "a": 1 }, "merged": { "a": 1, "c": 2 } })
        );
    }

    #[test]
    fn empty_yaml_is_empty_map() {
        assert_eq!(yaml_to_json("x.yaml", "").unwrap(), serde_json::json!({}));
    }

    #[test]
    fn privacy_paths() {
        assert_eq!(privacy_path("privacy/it.md", "it", "it"), "privacy/it.md");
        assert_eq!(privacy_path("privacy/it.md", "it", "en"), "privacy/en.md");
    }
}
