//! Assemble application state without starting an execution engine.
use crate::{
    BackendSettings, Error, Research, Result,
    adapter::orbit::{Compatibility, OrbitBackend},
    runtime::Application,
};
use std::path::Path;

impl Application {
    pub fn new(root: &Path, orbit: Option<OrbitBackend>, publication_ref: String) -> Result<Self> {
        if !publication_ref.starts_with("refs/remotes/") {
            return Err(Error::Invalid(
                "Publication target must be an explicit remote-tracking ref".into(),
            ));
        }
        Ok(Self {
            corpus: Research::open(root)?,
            orbit,
            publication_ref,
        })
    }

    pub fn configured(root: &Path, settings: BackendSettings) -> Result<Self> {
        let compatibility: Vec<Compatibility> =
            serde_json::from_str(include_str!("../assets/orbit-compatibility.json"))?;
        let orbit = OrbitBackend::new(settings.backend, compatibility)?;
        // Do not probe here: a backend outage must not prevent local capture/read.
        Self::new(root, Some(orbit), settings.publication_ref)
    }

    pub fn local(root: &Path) -> Result<Self> {
        Self::new(root, None, "refs/remotes/origin/agent-main".into())
    }
}

pub fn init_workspace(path: &Path) -> Result<serde_json::Value> {
    orbit_research_store::workspace::init(path)
}
