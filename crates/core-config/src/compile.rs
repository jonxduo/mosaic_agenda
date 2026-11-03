//! Compilation of a validated package into the binary bundle (PROGETTO.md §6.5).

use std::collections::BTreeMap;

use core_model::config::PackageConfig;
use core_model::eval::ConfigIndex;
use serde::{Deserialize, Serialize};

/// Version of the compiled layout; bumped on incompatible changes.
pub const COMPILED_FORMAT: u32 = 4;

/// The previous layout (uncompressed), still read: organizations created
/// before carry it in their history.
pub const FORMAT_UNCOMPRESSED: u32 = 3;

/// Ceiling for the decompressed size of a bundle.
const MAX_INFLATED: usize = 16 * 1024 * 1024;

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
    #[error("bundle sources cannot be loaded: {0}")]
    Sources(String),
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
    let compiled = CompiledConfig::new(package);
    let mut bytes = postcard::to_allocvec(&Header {
        format: COMPILED_FORMAT,
        core_version: compiled.core_version.clone(),
        sources: BTreeMap::new(),
    })?;
    let mut inner = postcard::to_allocvec(&sources)?;
    inner.extend(postcard::to_allocvec(&Body {
        package: &compiled.package,
        index: &compiled.index,
    })?);
    bytes.extend(miniz_oxide::deflate::compress_to_vec(&inner, 9));
    let hash = *blake3::hash(&bytes).as_bytes();
    Ok(Bundle { bytes, hash })
}

/// Layout (format 4): a header (format, core version, no sources), then,
/// compressed with deflate, the source files and the compiled package — a
/// published configuration must fit in one event. Format 3 had the sources
/// in the header and nothing compressed. postcard is not self-describing, so a newer core may not
/// read the compiled part of an older bundle (a field added to the model):
/// it then compiles the package again from the sources, which every
/// published version carries.
#[derive(Serialize, Deserialize)]
struct Header {
    format: u32,
    core_version: String,
    sources: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct Body<'a> {
    package: &'a PackageConfig,
    index: &'a ConfigIndex,
}

#[derive(Deserialize)]
struct OwnedBody {
    package: PackageConfig,
    index: ConfigIndex,
}

/// Decodes a bundle, checking its format version.
pub fn decode(bytes: &[u8]) -> Result<CompiledConfig, DecodeError> {
    let (mut header, rest) = postcard::take_from_bytes::<Header>(bytes)?;
    let inflated;
    let rest = match header.format {
        COMPILED_FORMAT => {
            inflated = miniz_oxide::inflate::decompress_to_vec_with_limit(rest, MAX_INFLATED)
                .map_err(|e| DecodeError::Sources(format!("cannot inflate: {e:?}")))?;
            let (sources, body) = postcard::take_from_bytes::<BTreeMap<String, String>>(&inflated)?;
            header.sources = sources;
            body
        }
        FORMAT_UNCOMPRESSED => rest,
        found => {
            return Err(DecodeError::Format {
                found,
                expected: COMPILED_FORMAT,
            });
        }
    };
    match postcard::from_bytes::<OwnedBody>(rest) {
        Ok(body) => Ok(CompiledConfig {
            format: COMPILED_FORMAT,
            core_version: header.core_version,
            package: body.package,
            index: body.index,
            sources: header.sources,
        }),
        Err(e) if header.sources.is_empty() => Err(e.into()),
        Err(_) => {
            // Recompiling is slow and the same bundle comes back at every
            // rebuild of the projection: keep the result by content.
            use std::collections::HashMap;
            use std::sync::{Mutex, OnceLock};
            static RECOMPILED: OnceLock<Mutex<HashMap<[u8; 32], CompiledConfig>>> = OnceLock::new();
            let key = *blake3::hash(bytes).as_bytes();
            let cache = RECOMPILED.get_or_init(Default::default);
            if let Some(c) = cache.lock().ok().and_then(|m| m.get(&key).cloned()) {
                return Ok(c);
            }
            let package = crate::load_files(&header.sources).map_err(|issues| {
                DecodeError::Sources(
                    issues
                        .first()
                        .map_or_else(String::new, |i| format!("{}: {}", i.file, i.message)),
                )
            })?;
            let mut compiled = CompiledConfig::new(package);
            compiled.sources = header.sources;
            if let Ok(mut m) = cache.lock() {
                m.insert(key, compiled.clone());
            }
            Ok(compiled)
        }
    }
}
