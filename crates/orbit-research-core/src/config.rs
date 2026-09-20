//! Operator configuration; bootstrap loads it and the Orbit adapter validates it.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackendConfig {
    pub executable: PathBuf,
    pub workspace: String,
    pub checkout: PathBuf,
    pub owner_machine_id: String,
}

/// Configuration selected by the operator at process startup. The certification
/// table is shipped with this app, not supplied by a tool or configuration file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendSettings {
    pub backend: BackendConfig,
    pub publication_ref: String,
}
