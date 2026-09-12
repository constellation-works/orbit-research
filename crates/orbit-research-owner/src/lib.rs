//! Owner-side authoring for canonical scientific records.
//!
//! This crate is the write path: an explicit Git checkout, immutable JSON appends under
//! `research/`, exact publication pins, and opt-in byte verification of retained datasets.
//! It authors JSON only — it never commits, pushes, schedules work, or reaches an Orbit
//! store — and every reference it resolves names an exact revision, never HEAD.

pub mod artifacts;
pub mod checkout;
pub mod git;
pub mod native;
pub mod owner;
pub mod time;

use thiserror::Error;

pub use artifacts::{
    ArtifactSource, FsArtifactResolver, SchemaCheck, VerifiedArtifact, verify_artifact,
};
pub use checkout::{Checkout, safe_path};
pub use git::{Git, SubprocessGit};
pub use native::{
    RecordKey, confirmation_errors, native_errors, native_reference_errors, protocol_errors,
    record_key, reference_key,
};
pub use orbit_research_contract::{GitRevision, Pin};
pub use owner::{
    NativeReceipt, OPERATIONS, Owner, OwnerConfig, reference, validate_native, write_json_new,
};
pub use time::{Clock, FixedClock, SystemClock, instant};

#[derive(Debug, Error)]
pub enum OwnerError {
    /// A refused scientific or structural request. The CLI reports this as `invalid-input`.
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Contract(#[from] orbit_research_contract::ContractError),
}

pub type Result<T> = std::result::Result<T, OwnerError>;

/// Fail closed with an explicit reason, mirroring the Python `require` guard.
pub(crate) fn require(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(OwnerError::Invalid(message.to_owned()))
    }
}
