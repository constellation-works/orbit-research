//! Operator-selected startup configuration.
use crate::adapter::orbit::BackendConfig;
use serde::Deserialize;

/// Configuration selected by the operator at process startup. The certification
/// table is shipped with this app, not supplied by a tool or configuration file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendSettings {
    pub backend: BackendConfig,
    pub publication_ref: String,
}
