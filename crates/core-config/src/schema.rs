//! JSON Schema of the package format, generated from `core-model`.

use core_model::config::PackageConfig;
use serde_json::Value;

/// The full schema: `PackageConfig` with one `$defs` entry per file type.
/// Built once: generating it is slow and it never changes in a process.
pub fn package_schema() -> Value {
    static SCHEMA: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    SCHEMA
        .get_or_init(|| {
            serde_json::to_value(schemars::schema_for!(PackageConfig)).unwrap_or(Value::Null)
        })
        .clone()
}

/// Pretty-printed schema, as committed in `schemas/package.schema.json`.
pub fn package_schema_pretty() -> String {
    let mut s = serde_json::to_string_pretty(&package_schema()).unwrap_or_default();
    s.push('\n');
    s
}

/// A schema validating one file: `$ref` to the file's definition, sharing `$defs`.
pub fn file_schema(definition: &str) -> Value {
    let root = package_schema();
    let defs = root.get("$defs").cloned().unwrap_or(Value::Null);
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$ref": format!("#/$defs/{definition}"),
        "$defs": defs,
    })
}
