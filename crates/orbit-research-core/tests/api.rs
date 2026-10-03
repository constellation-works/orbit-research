use orbit_research_core::api::Application;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

const SCHEMA: &[u8] = include_bytes!("fixtures/schema.json");

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git should be installed");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn fixture() -> (TempDir, Application) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir(root.join("_scripts")).unwrap();
    fs::write(root.join("_scripts/schema.json"), SCHEMA).unwrap();
    fs::write(root.join(".gitignore"), "/_data/**\n!/_data/**/\n")
        .expect("ignored operational bytes");
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::create_dir(root.join(directory)).unwrap();
    }
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "api-tests@example.invalid"]);
    git(root, &["config", "user.name", "API tests"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "fixture"]);
    let app = Application::local(root).unwrap();
    (temp, app)
}

fn create_r(app: &Application, request_key: &str) -> Value {
    app.call(
        "research.create",
        json!({
            "request_key": request_key,
            "kind": "R",
            "title": "A study",
            "body": "Question under study",
            "tags": ["api"],
        }),
    )
    .unwrap()
}

#[test]
fn confirmed_link_retries_preserve_the_original_task() {
    let (temp, app) = fixture();
    create_r(&app, "link-reservation");
    let preparation = app
        .link_intent("link-request", "R001")
        .expect("record link intent");
    assert!(preparation.is_new);
    assert!(preparation.link.task_id.is_none());

    let first = app
        .link_confirm("link-request", "task-first")
        .expect("first confirmation");
    let retry = app
        .link_confirm("link-request", "task-first")
        .expect("identical confirmation retry");
    assert_eq!(first.task_id, retry.task_id);

    let error = app
        .link_confirm("link-request", "task-competing")
        .expect_err("a confirmed request cannot be reassigned to another task");
    assert!(matches!(error, orbit_research_core::Error::Conflict(_)));
    let message = error.to_string();
    assert!(!message.contains("task-first"));
    assert!(!message.contains("task-competing"));
    let reopened = Application::local(temp.path()).expect("reopen fixture application");
    let recalled = reopened
        .link_intent("link-request", "R001")
        .expect("recall persisted link");
    assert!(!recalled.is_new);
    assert_eq!(recalled.link.task_id.as_deref(), Some("task-first"));
    let links = reopened
        .call("research.work_links", json!({}))
        .expect("list persisted links");
    assert_eq!(links.as_array().expect("link array").len(), 1);
    assert_eq!(links[0]["task_id"], "task-first");
}

#[test]
fn link_requests_share_confirmations_across_worktrees_without_git_on_path() {
    const PRIMARY: &str = "ORBIT_RESEARCH_LINK_NO_GIT_PRIMARY";
    const LINKED: &str = "ORBIT_RESEARCH_LINK_NO_GIT_LINKED";
    if let Some(primary) = std::env::var_os(PRIMARY) {
        let linked = std::env::var_os(LINKED).expect("linked worktree fixture path");
        let primary = Application::local(Path::new(&primary)).expect("open primary without Git");
        let linked = Application::local(Path::new(&linked)).expect("open worktree without Git");
        let intent = primary
            .link_intent("git-free-link", "R001")
            .expect("persist intent without spawning Git");
        assert!(intent.is_new);
        let recalled = linked
            .link_intent("git-free-link", "R001")
            .expect("worktree recalls the primary intent without spawning Git");
        assert!(!recalled.is_new);
        linked
            .link_confirm("git-free-link", "task-confirmed")
            .expect("confirm the shared intent without spawning Git");
        let links = primary
            .call("research.work_links", json!({}))
            .expect("list the shared confirmation without spawning Git");
        assert_eq!(links.as_array().expect("link array").len(), 1);
        assert_eq!(links[0]["task_id"], "task-confirmed");
        return;
    }

    let (temp, app) = fixture();
    create_r(&app, "git-free-link-reservation");
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    orbit_research_core::prepare_workspace_operations(temp.path())
        .expect("prepare process-free shared state");
    let linked_parent = tempfile::tempdir().expect("private linked worktree parent");
    let linked = linked_parent.path().join("worktree");
    git(
        temp.path(),
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            linked.to_str().expect("UTF-8 private worktree path"),
        ],
    );
    let output = Command::new(std::env::current_exe().expect("current test binary"))
        .args([
            "--exact",
            "link_requests_share_confirmations_across_worktrees_without_git_on_path",
        ])
        .env("PATH", "")
        .env(PRIMARY, temp.path())
        .env(LINKED, &linked)
        .output()
        .expect("run link operations in a child with an empty PATH");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "Git-free link regression must execute and pass: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    assert!(temp.path().join(".git/orbit-research-operations").is_file());
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    assert!(temp.path().join(".git/orbit-research-operations").is_dir());
    assert!(
        !linked.join(".git/orbit-research-operations").exists(),
        "linked worktrees must keep using the shared request log"
    );
}

#[test]
fn link_request_io_failures_identify_the_operation_and_path() {
    for block_lock in [false, true] {
        let (temp, app) = fixture();
        create_r(&app, "io-context-reservation");
        let log_root = temp
            .path()
            .canonicalize()
            .expect("canonical fixture root")
            .join(".git/orbit-research-operations");
        let (_operation, failing_path) = if block_lock {
            fs::create_dir(&log_root).expect("request log directory");
            let lock = log_root.join("lock");
            fs::create_dir(&lock).expect("directory blocking the lock file");
            ("open request-log lock", lock)
        } else {
            fs::write(&log_root, "preserve this file").expect("file blocking the log directory");
            ("create request-log directory", log_root.clone())
        };
        let error = match app.link_intent("io-context-link", "R001") {
            Err(error) => error,
            Ok(_) => panic!("blocked request log must fail before saving an intent"),
        };
        if !block_lock {
            let message = error.to_string();
            assert!(matches!(error, orbit_research_core::Error::Refused(_)));
            assert!(
                message.contains("Unrecognized request-storage layout"),
                "{message}"
            );
            assert!(
                message.contains(&failing_path.display().to_string()),
                "{message}"
            );
            assert_eq!(
                fs::read_to_string(log_root).expect("blocking file remains readable"),
                "preserve this file"
            );
            continue;
        }
        assert!(matches!(error, orbit_research_core::Error::Refused(_)));
        let message = error.to_string();
        assert!(message.contains("not an ordinary file"), "{message}");
        assert!(
            message.contains(&failing_path.display().to_string()),
            "{message}"
        );
    }
}

#[cfg(unix)]
#[test]
fn request_log_permission_errors_keep_io_kind_and_original_source() {
    use std::os::unix::fs::PermissionsExt;
    let (temp, app) = fixture();
    create_r(&app, "permission-reservation");
    app.link_intent("permission-link", "R001")
        .expect("initial intent");
    let root = temp.path().join(".git/orbit-research-operations");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o500))
        .expect("read-only request directory");
    let result = app.link_confirm("permission-link", "task-confirmed");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .expect("restore cleanup permissions");
    let orbit_research_core::Error::Io(error) =
        result.expect_err("confirmation cannot write into a read-only directory")
    else {
        panic!("permission failures retain the typed Io variant");
    };
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(std::error::Error::source(&error).is_some());
    assert!(
        error
            .to_string()
            .contains("create request-log temporary file")
    );
    assert!(
        app.link_intent("permission-link", "R001")
            .expect("recall unchanged intent")
            .link
            .task_id
            .is_none()
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn preparation_preserves_pending_intents_and_confirmed_task_links() {
    let (temp, app) = fixture();
    create_r(&app, "pending-reservation");
    let pending = app
        .link_intent("pending-request", "R001")
        .expect("legacy uncertain intent");
    app.link_intent("confirmed-request", "R001")
        .expect("legacy confirmed intent");
    app.link_confirm("confirmed-request", "task-original")
        .expect("legacy confirmation");
    let old = temp.path().join(".git/orbit-research-operations");
    let originals = fs::read_dir(&old)
        .expect("legacy files")
        .map(|entry| {
            let entry = entry.expect("entry");
            (
                entry.file_name(),
                fs::read(entry.path()).expect("original bytes"),
            )
        })
        .collect::<Vec<_>>();
    let prepared = orbit_research_core::prepare_workspace_operations(temp.path())
        .expect("prepare existing correlations");
    assert_eq!(prepared["changed"], true);
    let new = temp.path().join("_data/orbit-research-operations");
    for (name, bytes) in originals {
        assert_eq!(fs::read(new.join(name)).expect("moved bytes"), bytes);
    }
    let recalled = app
        .link_intent("pending-request", "R001")
        .expect("recall uncertain outcome");
    assert!(!recalled.is_new);
    assert_eq!(recalled.link.correlation_tag, pending.link.correlation_tag);
    assert!(recalled.link.task_id.is_none());
    app.link_confirm("pending-request", "task-resolved")
        .expect("confirm an adopted uncertain task");
    let conflict = app
        .link_confirm("confirmed-request", "task-competing")
        .expect_err("preserve the original confirmed task after migration");
    assert!(matches!(conflict, orbit_research_core::Error::Conflict(_)));
    let reopened = Application::local(temp.path()).expect("reopen prepared corpus");
    reopened
        .require_prepared_operations()
        .expect("plugin guard recognizes prepared state");
    assert_eq!(
        reopened
            .link_intent("confirmed-request", "R001")
            .expect("confirmed retry")
            .link
            .task_id
            .as_deref(),
        Some("task-original")
    );
    assert_eq!(
        reopened
            .call("research.work_links", json!({}))
            .expect("shared links")
            .as_array()
            .expect("array")
            .len(),
        2
    );
}

#[test]
fn local_capture_and_work_plans_need_no_backend() {
    let (temp, app) = fixture();
    let first = create_r(&app, "capture-r");
    let mut second = create_r(&app, "capture-r");
    assert_eq!(second["replayed"], true, "a retry says it was replayed");
    assert!(first.get("replayed").is_none());
    second.as_object_mut().unwrap().remove("replayed");
    assert_eq!(first, second, "request-key retry must be idempotent");
    assert!(!first["git_blob"].as_str().unwrap().is_empty());
    assert_eq!(first["id"], "R001");

    let investigation = app
        .call(
            "research.plan",
            json!({
                "shape": "investigation",
                "research_id":"R001",
                "objective":"Reproduce the baseline."
            }),
        )
        .unwrap();
    assert_eq!(
        investigation["context_files"],
        json!(["dir:research/R001-a-study"])
    );
    assert!(
        !investigation["acceptance_criteria"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let contribution = app
        .call(
            "research.plan",
            json!({
                "shape": "contribution",
                "research_id":"R001",
                "unit":"control-a",
                "objective":"Measure the control."
            }),
        )
        .unwrap();
    assert!(
        contribution["title"]
            .as_str()
            .unwrap()
            .contains("control-a")
    );

    let findings = temp
        .path()
        .join("research/R001-a-study/artifacts/control-a");
    fs::create_dir_all(&findings).unwrap();
    fs::write(findings.join("findings.md"), "Findings.\n").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-q", "-m", "contribution"]);
    let synthesis = app
        .call(
            "research.plan",
            json!({"shape": "synthesis", "research_id":"R001","units":["control-a"]}),
        )
        .unwrap();
    assert!(synthesis["title"].as_str().unwrap().contains("Synthesize"));
    assert!(temp.path().join(".git/orbit-research-writer").exists());
    assert!(
        !temp.path().join(".git/orbit-research-operations").exists(),
        "local capture and planning must not create Orbit operation state"
    );
    assert_eq!(git(temp.path(), &["rev-list", "--count", "HEAD"]), "3");
}

#[test]
fn operation_inputs_cannot_replace_fixed_root_or_declare_unknown_fields() {
    let (_temp, app) = fixture();
    let cases = [
        (
            "research.create",
            json!({"request_key":"r","kind":"Q","title":"Question","root":"/tmp"}),
        ),
        (
            "research.plan",
            json!({"shape":"investigation","research_id":"R001","objective":"objective","backend":"other"}),
        ),
    ];
    for (operation, input) in cases {
        let error = app.call(operation, input).unwrap_err().to_string();
        assert!(error.contains("unknown field"), "{operation}: {error}");
    }
}

#[test]
fn derived_constraints_are_enforced_before_mutation() {
    use orbit_research_core::application::Operation;
    let (temp, app) = fixture();
    for input in [
        json!({"request_key":"", "kind":"Q", "title":"Question"}),
        json!({"request_key":"bad-kind", "kind":"assessment", "title":"Question"}),
        json!({"request_key":"empty-title", "kind":"Q", "title":""}),
    ] {
        assert!(app.execute(Operation::Create, input).is_err());
    }
    assert_eq!(git(temp.path(), &["rev-list", "--count", "HEAD"]), "1");
    assert!(!temp.path().join(".git/orbit-research-writer").exists());
    assert!(app.execute(Operation::List, json!({})).is_ok());
}

#[test]
fn check_returns_a_compact_summary_and_list_keeps_its_snapshot_schema() {
    let (temp, app) = fixture();
    create_r(&app, "check-summary");
    let base_revision = git(temp.path(), &["rev-parse", "HEAD"]);
    let status_before = git(temp.path(), &["status", "--porcelain"]);

    let check = app.call("research.check", json!({})).unwrap();
    assert_eq!(
        check,
        json!({
            "valid": true,
            "base_revision": base_revision,
            "record_count": 1,
            "tag_count": 1,
        })
    );
    assert!(check.get("records").is_none());
    assert!(check.get("body").is_none());
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), base_revision);
    assert_eq!(git(temp.path(), &["status", "--porcelain"]), status_before);

    let list = app.call("research.list", json!({})).unwrap();
    assert_eq!(list["revision"], base_revision);
    assert_eq!(list["records"].as_array().unwrap().len(), 1);
    assert!(
        list["records"][0]["body"]
            .as_str()
            .expect("record body")
            .contains("Question under study")
    );
    assert_eq!(list["tags"], json!(["api"]));
}

#[test]
fn check_rejects_an_invalid_corpus_without_changing_git_state() {
    let (temp, app) = fixture();
    fs::write(
        temp.path().join("questions/Q001-invalid.md"),
        "not canonical frontmatter\n",
    )
    .unwrap();
    let head_before = git(temp.path(), &["rev-parse", "HEAD"]);
    let status_before = git(temp.path(), &["status", "--porcelain"]);

    let error = app.call("research.check", json!({})).unwrap_err();
    assert!(error.to_string().contains("missing frontmatter"));
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), head_before);
    assert_eq!(git(temp.path(), &["status", "--porcelain"]), status_before);
}

#[test]
fn work_links_list_oldest_first_whatever_their_keys() {
    use std::time::{Duration, SystemTime};
    let (temp, app) = fixture();
    create_r(&app, "links-reservation");
    for key in ["zeta", "alpha", "mid"] {
        app.link_intent(key, "R001").expect("record link intent");
    }
    // Directory order and key order are both arbitrary; the listing follows
    // when each correlation was last written.
    let now = SystemTime::now();
    let log = temp.path().join(".git/orbit-research-operations");
    for (key, age) in [("zeta", 30), ("alpha", 20), ("mid", 10)] {
        for entry in fs::read_dir(&log).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json")
                && fs::read_to_string(&path)
                    .unwrap()
                    .contains(&format!("\"request_key\": \"{key}\""))
            {
                let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
                file.set_modified(now - Duration::from_secs(age)).unwrap();
            }
        }
    }
    let links = app.call("research.work_links", json!({})).unwrap();
    let keys: Vec<_> = links
        .as_array()
        .unwrap()
        .iter()
        .map(|link| link["request_key"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(keys, ["zeta", "alpha", "mid"]);
}

#[test]
fn revise_with_no_fields_says_there_is_nothing_to_change() {
    let (_temp, app) = fixture();
    let created = app
        .call(
            "research.create",
            json!({"request_key": "q", "kind": "Q", "title": "A question", "body": "b"}),
        )
        .unwrap();
    let error = app
        .call(
            "research.revise",
            json!({"id": "Q001", "expected_blob": created["git_blob"]}),
        )
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("Nothing to change"), "{error}");
}
