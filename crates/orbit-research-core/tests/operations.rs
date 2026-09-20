use orbit_research_core::{
    Research,
    backend::{BackendConfig, Compatibility, OrbitBackend},
};
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[test]
fn preflight_refusal_is_retryable_but_unknown_submission_is_not_repeated() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("corpus");
    fs::create_dir_all(root.join("_scripts")).expect("schema dir");
    fs::write(
        root.join("_scripts/schema.json"),
        include_bytes!("fixtures/schema.json"),
    )
    .expect("schema");
    for dir in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(dir)).expect("directory");
    }
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "fixture@example.invalid"]);
    git(&root, &["config", "user.name", "Fixture"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "fixture"]);
    let corpus = Research::open(&root).expect("corpus");
    corpus
        .reserve("r", "R", "Study", "Question", vec![], vec![])
        .expect("reserve");
    let plan = corpus
        .investigation("R001", "Check a fixture.")
        .expect("plan");
    let executable = temp.path().join("fake-orbit");
    let submissions = temp.path().join("submissions");
    let identity = serde_json::json!({
        "registered": true,
        "workspace": { "id": "ws_fixture", "owner_machine_id": "hm_fixture" },
        "checkout": { "repo_root": root.canonicalize().expect("root") },
    })
    .to_string();
    fs::write(
        &executable,
        format!(
            r#"#!/bin/sh
case "$*" in
 *"--version"*) printf '%s\n' 'orbit fixture' ;;
 *"workspace show"*) printf '%s\n' {} ;;
 *"orbit.task.add"*) printf '%s\n' '{{"id":"T1","status":"proposed"}}' ;;
 *"orbit.task.show"*) printf '%s\n' '{{"id":"T1","status":"backlog","job_run_id":null}}' ;;
 *"run ship"*) printf '%s\n' submitted >> {}; printf '%s\n' 'unknown reply' ;;
 *) exit 3 ;;
esac
"#,
            quote(&identity),
            quote(&submissions.to_string_lossy())
        ),
    )
    .expect("script");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).expect("permissions");
    let digest = format!(
        "{:x}",
        Sha256::digest(fs::read(&executable).expect("script bytes"))
    );
    let config = BackendConfig {
        executable,
        workspace: "ws_fixture".into(),
        checkout: root.clone(),
        owner_machine_id: "hm_fixture".into(),
    };
    let make_backend = |allow_ship| {
        OrbitBackend::new(
            config.clone(),
            vec![Compatibility {
                binary_sha256: digest.clone(),
                version: "orbit fixture".into(),
                platform: std::env::consts::OS.into(),
                operations: if allow_ship {
                    vec!["observe".into(), "task_add".into(), "ship".into()]
                } else {
                    vec!["observe".into(), "task_add".into()]
                },
            }],
        )
        .expect("backend")
    };
    let blocked = make_backend(false);
    corpus
        .link_work(&blocked, "work", "Check fixture", "luna", &plan)
        .expect("link");
    let error = corpus
        .dispatch_work(&blocked, "work", "agent-main")
        .expect_err("uncertified dispatch");
    assert!(error.to_string().contains("not certified for ship"));
    assert!(!corpus.work_links().expect("links")[0].dispatch_attempted);
    assert!(!submissions.exists());
    let admitted = make_backend(true);
    assert!(
        corpus
            .dispatch_work(&admitted, "work", "--complete")
            .is_err()
    );
    assert!(!corpus.work_links().expect("links")[0].dispatch_attempted);
    assert!(
        corpus
            .dispatch_work(&admitted, "work", "agent-main")
            .is_err()
    );
    assert!(corpus.work_links().expect("links")[0].dispatch_attempted);
    let error = corpus
        .dispatch_work(&admitted, "work", "agent-main")
        .expect_err("unknown remains unresolved");
    assert!(error.to_string().contains("outcome is unknown"));
    assert_eq!(
        fs::read_to_string(submissions)
            .expect("submission evidence")
            .lines()
            .count(),
        1
    );
}
