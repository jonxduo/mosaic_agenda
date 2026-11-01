//! Domain-free model of the core.
//!
//! This crate defines:
//! - the source configuration types of a package ([`config`]), from which the
//!   JSON Schema in `schemas/package.schema.json` is generated;
//! - the declarative condition language ([`cond::Cond`]) and audience selectors
//!   ([`audience::Selector`]);
//! - the closed set of elementary permissions ([`permission::Permission`]);
//! - a minimal organization state with date-based history ([`state::OrgState`])
//!   and the pure functions that evaluate conditions and resolve audiences on
//!   it ([`eval`]).
//!
//! Nothing here refers to a specific domain: every concept comes from packages.

pub mod audience;
pub mod cond;
pub mod config;
pub mod eval;
pub mod id;
pub mod permission;
pub mod scalar;
pub mod serde_util;
pub mod state;
pub mod strings;

pub use audience::{AudienceDef, Selector, SystemAudience};
pub use cond::{CompareOp, Comparison, Cond, Expr};
pub use id::{Id, PersonId, UnitId};
pub use permission::{Permission, PermissionGrant, PermissionTarget};
pub use scalar::Scalar;
pub use strings::{Slug, TextKey};

/// Version of the core's configuration model, independent of crate versions.
/// Packages declare a `core_compat` range against it.
pub const CORE_VERSION: &str = "1.0.0";
