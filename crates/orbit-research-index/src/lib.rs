//! Disposable, fail-closed projection of explicitly supplied owner documents.
//!
//! No authoring, fetch, importer execution, or operational task state lives here.
//! Reconciliation is derived separately; original owner records and verdicts are never
//! edited. Ports `src/orbit_research/index.py` and `src/orbit_research/browser.py`.

mod browse;
mod build;
mod config;
mod read;
mod rebuild;

use thiserror::Error;

pub use browse::{export_browser, local_url, ExportOutcome};
pub use build::{build_projection, Projection};
pub use config::{guard_output, load_config, IndexConfig};
pub use read::{read_index, trace};
pub use rebuild::{rebuild, RebuildOutcome};

#[derive(Debug, Clone)]
pub struct Problem {
    pub source: String,
    pub reason: String,
}

impl Problem {
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::json!({"source": self.source, "reason": self.reason})
    }
}

#[derive(Debug, Error)]
pub enum IndexError {
    /// A refused request or a malformed document. The CLI reports this as `invalid-input`.
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Contract(#[from] orbit_research_contract::ContractError),
    #[error("{0}")]
    Owner(#[from] orbit_research_owner::OwnerError),
    #[error("{0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Fail-closed rebuild refusal: one or more supplied documents are invalid. The previous
    /// database, if any, is left untouched.
    #[error("{}", problems_message(.0))]
    Build(Vec<Problem>),
}

fn problems_message(problems: &[Problem]) -> String {
    problems
        .iter()
        .map(|problem| format!("{}: {}", problem.source, problem.reason))
        .collect::<Vec<_>>()
        .join("; ")
}

impl IndexError {
    pub fn problems(&self) -> Option<&[Problem]> {
        match self {
            IndexError::Build(problems) => Some(problems),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, IndexError>;

pub(crate) fn require(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(IndexError::Invalid(message.into()))
    }
}

pub(crate) fn digest_bytes(data: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    format!("sha256:{:x}", Sha256::digest(data))
}

pub(crate) fn file_digest(path: &std::path::Path) -> Result<String> {
    Ok(digest_bytes(&std::fs::read(path)?))
}

pub(crate) fn text<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}
