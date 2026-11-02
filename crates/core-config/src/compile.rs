//! Compilation of a validated package into the binary bundle (PROGETTO.md §6.5).

use std::collections::BTreeMap;

use core_model::config::PackageConfig;
use core_model::eval::ConfigIndex;
use serde::{Deserialize, Serialize};

/// Version of the compiled layout; bumped on incompatible changes.
pub const COMPILED_FORMAT: u32 = 2;

/// What the engine uses at runtime: the validated package (anchors expanded,
/// texts flattened) plus derived lookup tables.
///
/// References stay as slugs rather than numeric indices: the bundle is small
/// and slugs stay stable across versions and migrations (see docs/DECISIONS.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompiledConfig {
    pub format: u32,
    pub core_version: String,
    pub package: PackageConfig,
    pub index: ConfigIndex,
    /// The package files as written (comments included), keyed by their path
    /// in the package: what the configuration editor starts from (§12.4).
    /// Empty for bundles compiled without them (tests).
    pub sources: BTreeMap<String, String>,
}

impl CompiledConfig {
    pub fn new(package: PackageConfig) -> Self {
        let index = ConfigIndex::build(&package.qualifications, &package.roles, &package.audiences);
        Self {
            format: COMPILED_FORMAT,
            core_version: core_model::CORE_VERSION.to_owned(),
            package,
            index,
            sources: BTreeMap::new(),
        }
    }
}

/// Serialized compiled configuration and its BLAKE3 hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub bytes: Vec<u8>,
    pub hash: [u8; 32],
}

impl Bundle {
    pub fn hash_hex(&self) -> String {
        blake3::Hash::from_bytes(self.hash).to_hex().to_string()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("bundle cannot be decoded: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("bundle format {found} is not supported (expected {expected})")]
    Format { found: u32, expected: u32 },
}

/// Compiles a package that already passed validation.
pub fn compile(package: PackageConfig) -> Result<Bundle, postcard::Error> {
    compile_with_sources(package, BTreeMap::new())
}

/// Like [`compile`], keeping the source files for the configuration editor.
pub fn compile_with_sources(
    package: PackageConfig,
    sources: BTreeMap<String, String>,
) -> Result<Bundle, postcard::Error> {
    let mut compiled = CompiledConfig::new(package);
    compiled.sources = sources;
    let bytes = postcard::to_allocvec(&compiled)?;
    let hash = *blake3::hash(&bytes).as_bytes();
    Ok(Bundle { bytes, hash })
}

/// Decodes a bundle, checking its format version.
pub fn decode(bytes: &[u8]) -> Result<CompiledConfig, DecodeError> {
    let compiled: CompiledConfig = postcard::from_bytes(bytes)?;
    if compiled.format != COMPILED_FORMAT {
        return Err(DecodeError::Format {
            found: compiled.format,
            expected: COMPILED_FORMAT,
        });
    }
    Ok(compiled)
}
