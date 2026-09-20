//! Opt-in read-only verification against an explicitly selected installed backend.
//! This probe does not certify mutation operations or inject operator authority.
use orbit_research_core::backend::{BackendConfig, Compatibility, OrbitBackend};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("Set {name} explicitly for this probe"))
}

#[test]
#[ignore = "Requires an explicitly selected installed Orbit and registered workspace"]
fn installed_backend_observation_contract() {
    let executable = PathBuf::from(required("ORBIT_RESEARCH_TEST_EXECUTABLE"));
    let version = required("ORBIT_RESEARCH_TEST_VERSION");
    let expected_digest = required("ORBIT_RESEARCH_TEST_SHA256");
    let actual_digest = format!(
        "{:x}",
        Sha256::digest(fs::read(&executable).expect("read binary"))
    );
    assert_eq!(
        actual_digest, expected_digest,
        "installed binary changed before probe"
    );
    let workspace = required("ORBIT_RESEARCH_TEST_WORKSPACE");
    let owner = required("ORBIT_RESEARCH_TEST_OWNER");
    let backend = OrbitBackend::new(
        BackendConfig {
            executable,
            checkout: PathBuf::from(required("ORBIT_RESEARCH_TEST_CHECKOUT")),
            workspace: workspace.clone(),
            owner_machine_id: owner.clone(),
        },
        vec![Compatibility {
            binary_sha256: expected_digest,
            version,
            platform: env::consts::OS.into(),
            operations: vec!["observe".into()],
        }],
    )
    .expect("explicit backend");
    let identity = backend.inspect().expect("workspace identity");
    assert_eq!(identity["workspace"]["id"], workspace);
    assert_eq!(identity["workspace"]["owner_machine_id"], owner);
    let task_id = required("ORBIT_RESEARCH_TEST_TASK");
    let task = backend.task(&task_id).expect("structured task observation");
    assert_eq!(task["id"], task_id);
    let run_id = required("ORBIT_RESEARCH_TEST_RUN");
    let run = backend.run(&run_id).expect("structured run observation");
    assert_eq!(run["run_id"], run_id);
}
