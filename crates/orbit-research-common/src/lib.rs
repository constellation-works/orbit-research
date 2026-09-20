//! Leaf contracts shared by persistence, application and presentation layers.
//! No filesystem operations, Git invocation, runtime, or workspace-crate dependencies.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod legacy_contract;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Yaml(#[from] serde_yaml::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

/// A disposable view of a canonical Markdown file. No scientific state is stored here.
#[derive(Debug, Clone, Serialize)]
pub struct Record {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub metadata: Value,
    pub body: String,
    pub content_sha256: String,
    pub git_blob: String,
}
#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub revision: String,
    pub records: Vec<Record>,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Reservation {
    pub id: String,
    pub path: String,
    pub commit: String,
    pub request_digest: String,
}
