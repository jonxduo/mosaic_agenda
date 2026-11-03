//! Loading, validation and compilation of configuration packages
//! (PROGETTO.md §6).
//!
//! Pipeline: YAML (anchors expanded, top-level `x-` keys dropped) →
//! JSON Schema check → typed deserialization → semantic validation →
//! [`CompiledConfig`] serialized with postcard and hashed with BLAKE3.

mod compat;
mod compile;
mod issue;
mod loader;
mod migrate;
pub mod schema;
mod validate;

pub use compat::{CompatError, version_matches};
pub use compile::{
    Bundle, COMPILED_FORMAT, CompiledConfig, DecodeError, compile, compile_with_sources, decode,
};
pub use issue::{Issue, Severity};
pub use loader::{PACKAGE_FILES, load_files, load_package, source_files};
pub use migrate::{Change, check_migrations, diff, renames};
pub use validate::{template_names, validate};

use std::path::Path;

use core_model::config::PackageConfig;

/// Parses a `package.yaml` text alone (used by tooling that only needs the manifest).
pub fn parse_manifest(text: &str) -> Result<core_model::config::Manifest, Issue> {
    let json = loader::yaml_to_json("package.yaml", text)?;
    serde_path_to_error::deserialize(json)
        .map_err(|e| Issue::error("package.yaml", e.path().to_string(), e.inner().to_string()))
}

/// Loads and validates a package directory. On success returns the package
/// and its warnings; on failure every error (and warning) found.
pub fn load_and_validate(dir: &Path) -> Result<(PackageConfig, Vec<Issue>), Vec<Issue>> {
    let pkg = load_package(dir)?;
    let issues = validate(&pkg, Some(dir));
    if issues.iter().any(|i| i.severity == Severity::Error) {
        Err(issues)
    } else {
        Ok((pkg, issues))
    }
}
