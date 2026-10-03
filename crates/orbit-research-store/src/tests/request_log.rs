use crate::{Error, corpus::Corpus, request_log::prepare_workspace_operations};
use fs2::FileExt;
use serde_json::{Value, json};
use std::os::unix::fs::{MetadataExt, symlink};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
    time::Duration,
};
use tempfile::TempDir;

const KEY: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BYTES: &[u8] = b"{ \"task_id\": null, \"note\": \"preserve uncertain outcome\" }\n";
const STATE: &str = "_data/orbit-research-operations";

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("fixture Git command");
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().into()
}

fn fixture() -> TempDir {
    let temp = tempfile::tempdir().expect("private operations fixture");
    let root = temp.path();
    for directory in [
        "_scripts",
        "_data",
        "questions",
        "hypotheses",
        "theories",
        "research",
    ] {
        fs::create_dir(root.join(directory)).expect("fixture directory");
    }
    fs::write(
        root.join("_scripts/schema.json"),
        include_bytes!("fixtures/schema.json"),
    )
    .expect("owner contract");
    fs::write(
        root.join(".gitignore"),
        "_data/**\n!_data/**/\n!_data/**/manifest.json\n",
    )
    .expect("owner ignore rules");
    git(root, &["init", "-q"]);
    git(
        root,
        &["config", "user.email", "operations@example.invalid"],
    );
    git(root, &["config", "user.name", "Operations fixture"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "fixture"]);
    temp
}

fn legacy(root: &Path) -> PathBuf {
    let corpus = Corpus::open(root).expect("open legacy fixture");
    let log = corpus.request_log().expect("legacy request log");
    log.save(KEY, &json!({"task_id":null}))
        .expect("legacy intent");
    drop(log);
    let legacy = root.join(".git/orbit-research-operations");
    fs::write(legacy.join(format!("{KEY}.json")), BYTES).expect("exact legacy bytes");
    legacy
}

#[test]
fn preparation_preserves_bytes_lock_inode_and_shared_worktree_visibility() {
    let temp = fixture();
    let root = temp.path();
    let old = legacy(root);
    let old_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(old.join("lock"))
        .expect("old client descriptor");
    let inode = old_lock.metadata().expect("old lock metadata").ino();
    let head = git(root, &["rev-parse", "HEAD"]);
    let index = fs::read(root.join(".git/index")).expect("original index");
    let first = prepare_workspace_operations(root).expect("explicit preparation");
    assert_eq!(first["prepared"], true);
    assert_eq!(first["changed"], true);
    assert_eq!(first["layout_version"], 1);
    let state = root.join(STATE);
    assert!(
        old.is_file(),
        "old clients encounter an ordinary marker without a missing-path gap"
    );
    assert_eq!(
        fs::read(state.join(format!("{KEY}.json"))).expect("moved receipt"),
        BYTES
    );
    assert_eq!(
        fs::metadata(state.join("lock")).expect("moved lock").ino(),
        inode
    );
    assert_eq!(
        prepare_workspace_operations(root).expect("repeat preparation")["changed"],
        false
    );
    assert_eq!(git(root, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(root.join(".git/index")).expect("unchanged index"),
        index
    );
    assert_eq!(git(root, &["status", "--porcelain"]), "");

    // A descriptor opened by an old client before preparation still locks the
    // exact file new clients use, while its old path refuses reads and saves.
    old_lock
        .lock_exclusive()
        .expect("old descriptor acquires moved lock");
    assert!(fs::create_dir_all(&old).is_err());
    assert!(tempfile::NamedTempFile::new_in(&old).is_err());
    assert!(fs::read(old.join(format!("{KEY}.json"))).is_err());
    let linked_parent = tempfile::tempdir().expect("private worktree parent");
    let linked = linked_parent.path().join("checkout");
    git(
        root,
        &[
            "worktree",
            "add",
            "--detach",
            "-q",
            linked.to_str().expect("UTF-8 worktree path"),
        ],
    );
    let (started_send, started) = mpsc::channel();
    let (done_send, done) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let corpus = Corpus::open(&linked).expect("linked corpus");
        started_send.send(()).expect("signal reader start");
        let log = corpus.request_log().expect("linked shared request log");
        done_send
            .send(log.read::<Value>(KEY).expect("shared uncertain intent"))
            .expect("send shared value");
    });
    started
        .recv_timeout(Duration::from_secs(2))
        .expect("reader started");
    assert!(
        done.recv_timeout(Duration::from_millis(100)).is_err(),
        "new clients must contend on the moved old lock inode"
    );
    old_lock.unlock().expect("release old descriptor");
    let value = done
        .recv_timeout(Duration::from_secs(5))
        .expect("linked read resumes")
        .expect("preserved intent");
    assert!(value["task_id"].is_null());
    thread.join().expect("reader completed");
}

#[test]
fn preparation_recovers_an_owned_staging_marker_without_copying() {
    let temp = fixture();
    let root = temp.path();
    let old = legacy(root);
    prepare_workspace_operations(root).expect("obtain versioned layout fixture");
    let marker = fs::read(&old).expect("published marker");
    fs::remove_file(&old).expect("private fixture reversal");
    fs::rename(root.join(STATE), &old).expect("restore complete legacy directory");
    fs::write(root.join(STATE), marker).expect("model durable marker before atomic exchange");
    let inode = fs::metadata(old.join("lock"))
        .expect("staged old lock")
        .ino();
    let result = prepare_workspace_operations(root).expect("resume staged preparation");
    assert_eq!(result["changed"], true);
    assert_eq!(
        fs::metadata(root.join(STATE).join("lock"))
            .expect("recovered lock")
            .ino(),
        inode
    );
    assert_eq!(
        fs::read(root.join(STATE).join(format!("{KEY}.json"))).expect("recovered original bytes"),
        BYTES
    );
    assert_eq!(
        prepare_workspace_operations(root).expect("idempotent recovery")["changed"],
        false
    );
}

#[test]
fn failed_exchange_preserves_original_log_and_cleans_only_owned_markers() {
    let temp = fixture();
    let root = temp.path();
    legacy(root);
    let before = bytes(root);
    let error = crate::request_log_layout::prepare_with_exchange(root, |_, _| {
        Err(Error::Refused(
            "Filesystem does not support atomic exchange".into(),
        ))
    })
    .expect_err("unsupported exchange must refuse without copying");
    assert!(matches!(error, Error::Refused(_)));
    assert_eq!(
        bytes(root),
        before,
        "failed transition removes only its own staged marker and ledger"
    );
    assert_eq!(
        prepare_workspace_operations(root).expect("retry with real OS primitive")["changed"],
        true
    );
}

#[test]
fn reader_waiting_on_legacy_descriptor_retries_the_published_layout() {
    let temp = fixture();
    let root = temp.path();
    let old = legacy(root);
    let metadata = fs::metadata(old.join("lock")).expect("legacy lock identity");
    let (send, recv) = mpsc::channel();
    let reader_root = root.to_owned();
    let mut reader = None;
    let prepared = crate::request_log_layout::prepare_with_exchange(root, |source, target| {
        reader = Some(std::thread::spawn(move || {
            let corpus = Corpus::open(&reader_root)?;
            let mut send = Some(send);
            let log = corpus.request_log_with_open_observer(|file| {
                if let Some(send) = send.take() {
                    let metadata = file.metadata().expect("observed lock metadata");
                    send.send((metadata.dev(), metadata.ino()))
                        .expect("report actual opened lock identity");
                }
            })?;
            log.read::<Value>(KEY)
        }));
        let opened = recv.recv_timeout(Duration::from_secs(5)).map_err(|error| {
            Error::Internal(format!("Reader did not open the old lock: {error}"))
        })?;
        if opened != (metadata.dev(), metadata.ino()) {
            return Err(Error::Internal("Reader opened a different lock".into()));
        }
        crate::request_log_layout::exchange(source, target)
    });
    // Join before asserting preparation, so a failed handshake never removes
    // the private corpus while the reader still owns its descriptor.
    let read = reader
        .expect("reader thread")
        .join()
        .expect("reader completed");
    prepared.expect("prepare while a real reader is blocked on the legacy descriptor");
    assert!(
        read.expect("reader retries the published layout")
            .expect("existing intent")["task_id"]
            .is_null()
    );
}

#[test]
fn request_log_refuses_a_replaced_lock_after_opening_its_descriptor() {
    let temp = fixture();
    let root = temp.path();
    let old = legacy(root);
    let lock = old.join("lock");
    let corpus = Corpus::open(root).expect("corpus before lock replacement");
    let error = corpus
        .request_log_with_open_observer(|file| {
            let original = file.metadata().expect("opened lock identity");
            fs::remove_file(&lock).expect("replace the path while old descriptor remains open");
            fs::write(&lock, "foreign replacement").expect("replacement lock");
            assert_ne!(
                original.ino(),
                fs::metadata(&lock).expect("replacement identity").ino()
            );
        })
        .err()
        .expect("a stale lock must never authorize a callback or persistence");
    assert!(matches!(error, Error::Refused(_)));
    assert!(error.to_string().contains("lock changed while acquiring"));
    assert_eq!(
        fs::read(old.join(format!("{KEY}.json"))).expect("unchanged original receipt"),
        BYTES
    );
    assert_eq!(
        fs::read(lock).expect("preserved replacement"),
        b"foreign replacement"
    );
}

fn bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(path: &Path, values: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let metadata = fs::symlink_metadata(path).expect("fixture entry metadata");
        if metadata.file_type().is_symlink() {
            values.insert(
                path.into(),
                fs::read_link(path)
                    .expect("fixture symlink")
                    .to_string_lossy()
                    .as_bytes()
                    .to_vec(),
            );
        } else if metadata.is_dir() {
            values.insert(path.into(), Vec::new());
            for entry in fs::read_dir(path).expect("fixture directory") {
                collect(&entry.expect("entry").path(), values);
            }
        } else {
            values.insert(path.into(), fs::read(path).expect("fixture file"));
        }
    }
    let mut values = BTreeMap::new();
    collect(root, &mut values);
    values
}

#[test]
fn unsafe_conflicting_and_tracked_targets_refuse_without_changes() {
    for case in [
        "foreign_target",
        "unknown_version",
        "unknown_path",
        "unknown_field",
        "foreign_entry",
        "manifest",
        "source_symlink",
        "entry_symlink",
        "lock_symlink",
        "target_symlink",
        "ancestor_symlink",
        "tracked_missing",
    ] {
        let temp = fixture();
        let root = temp.path();
        let old = legacy(root);
        let state = root.join(STATE);
        let outside = tempfile::tempdir().expect("outside fixture");
        fs::write(outside.path().join("sentinel"), "preserve outside").expect("outside sentinel");
        match case {
            "foreign_target" => {
                fs::create_dir(&state).expect("target");
                fs::write(state.join("owned-by-someone-else"), "preserve").expect("foreign file");
            }
            "unknown_version" => {
                fs::write(
                    &state,
                    b"{\"layout_version\":2,\"state_path\":\"_data/orbit-research-operations\"}",
                )
                .expect("unknown version");
            }
            "unknown_path" => {
                fs::write(
                    &state,
                    b"{\"layout_version\":1,\"state_path\":\"../outside\"}",
                )
                .expect("unknown path");
            }
            "unknown_field" => {
                fs::write(&state, b"{\"layout_version\":1,\"state_path\":\"_data/orbit-research-operations\",\"extra\":true}").expect("unknown field");
            }
            "foreign_entry" => {
                fs::write(old.join("foreign.json"), "{}").expect("foreign entry");
            }
            "manifest" => {
                fs::write(old.join("manifest.json"), "{}").expect("manifest entry");
            }
            "source_symlink" => {
                fs::rename(&old, outside.path().join("log")).expect("move source");
                symlink(outside.path().join("log"), &old).expect("source symlink");
            }
            "entry_symlink" => {
                fs::remove_file(old.join(format!("{KEY}.json"))).expect("remove receipt");
                symlink(
                    outside.path().join("sentinel"),
                    old.join(format!("{KEY}.json")),
                )
                .expect("entry symlink");
            }
            "lock_symlink" => {
                fs::remove_file(old.join("lock")).expect("remove lock");
                symlink(outside.path().join("sentinel"), old.join("lock")).expect("lock symlink");
            }
            "target_symlink" => {
                symlink(outside.path(), &state).expect("target symlink");
            }
            "ancestor_symlink" => {
                fs::remove_dir(root.join("_data")).expect("empty data directory");
                symlink(outside.path(), root.join("_data")).expect("ancestor symlink");
            }
            "tracked_missing" => {
                fs::create_dir(&state).expect("target directory");
                fs::write(state.join("tracked.json"), "{}").expect("tracked fixture");
                git(
                    root,
                    &["add", "-f", "_data/orbit-research-operations/tracked.json"],
                );
                fs::remove_dir_all(&state).expect("missing working copy");
            }
            _ => unreachable!(),
        }
        let before = bytes(root);
        let outside_before = bytes(outside.path());
        assert!(
            matches!(prepare_workspace_operations(root), Err(Error::Refused(_))),
            "{case}"
        );
        assert_eq!(bytes(root), before, "{case} mutated the selected corpus");
        assert_eq!(
            bytes(outside.path()),
            outside_before,
            "{case} mutated outside state"
        );
    }
}

#[test]
fn nonordinary_markers_and_locks_refuse_without_blocking() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, time::Instant};
    for role in ["legacy", "ledger", "lock", "entry"] {
        let temp = fixture();
        let root = temp.path();
        let old = legacy(root);
        let path = match role {
            "legacy" => {
                fs::remove_dir_all(&old).expect("remove private source");
                old
            }
            "ledger" => {
                prepare_workspace_operations(root).expect("prepared fixture");
                let path = root.join(STATE).join(".layout");
                fs::remove_file(&path).expect("remove ledger");
                path
            }
            "lock" => {
                let path = old.join("lock");
                fs::remove_file(&path).expect("remove lock");
                path
            }
            "entry" => {
                let path = old.join(format!("{KEY}.json"));
                fs::remove_file(&path).expect("remove entry");
                path
            }
            _ => unreachable!(),
        };
        let cpath = CString::new(path.as_os_str().as_bytes()).expect("FIFO fixture path");
        // The selected name is private and absent; no external FIFO is opened.
        assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
        let start = Instant::now();
        let corpus = Corpus::open(root).expect("FIFO fixture corpus");
        let refused = if role == "entry" {
            let log = corpus.request_log().expect("ordinary log lock");
            matches!(log.read::<Value>(KEY), Err(Error::Refused(_)))
        } else {
            matches!(corpus.request_log(), Err(Error::Refused(_)))
        };
        assert!(refused, "{role}");
        assert!(start.elapsed() < Duration::from_secs(2), "{role} blocked");
        assert!(
            matches!(prepare_workspace_operations(root), Err(Error::Refused(_))),
            "prepare {role}"
        );
    }
}

fn prepared_fixture() -> TempDir {
    let temp = fixture();
    legacy(temp.path());
    prepare_workspace_operations(temp.path()).expect("prepared fixture");
    temp
}

#[test]
fn a_prepared_corpus_missing_its_state_directory_names_the_path_and_the_fix() {
    let temp = prepared_fixture();
    let root = temp.path();
    fs::remove_dir_all(root.join(STATE)).expect("delete state");
    let corpus = Corpus::open(root).expect("open corpus");
    for error in [
        corpus.request_log().err().expect("request log refuses"),
        corpus
            .require_prepared_operations()
            .expect_err("link refuses"),
    ] {
        assert!(matches!(error, Error::Refused(_)), "{error:?}");
        let message = error.to_string();
        assert!(!message.contains("os error"), "{message}");
        assert!(message.contains(STATE), "{message}");
        assert!(
            message.contains(&format!(
                "`orbit-research workspace prepare-operations {}`",
                root.canonicalize().unwrap().display()
            )),
            "{message}"
        );
    }
}

#[test]
fn preparation_recreates_a_state_directory_deleted_under_its_marker() {
    let temp = prepared_fixture();
    let root = temp.path();
    let head = git(root, &["rev-parse", "HEAD"]);
    let index = fs::read(root.join(".git/index")).expect("index");
    let marker = fs::read(root.join(".git/orbit-research-operations")).expect("marker");
    fs::remove_dir_all(root.join(STATE)).expect("delete state");

    let receipt = prepare_workspace_operations(root).expect("recreate");
    assert_eq!(receipt["prepared"], true);
    assert_eq!(receipt["changed"], true);
    assert_eq!(receipt["recreated"], true);
    assert!(root.join(STATE).join(".layout").is_file());
    assert_eq!(
        fs::metadata(root.join(STATE)).unwrap().mode() & 0o777,
        0o700
    );
    // The layout marker in Git metadata, HEAD, the index and the tree are untouched.
    assert_eq!(
        fs::read(root.join(".git/orbit-research-operations")).unwrap(),
        marker
    );
    assert_eq!(git(root, &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(git(root, &["status", "--porcelain"]), "");
    // The request log works again, empty: the earlier records were lost with the directory.
    let corpus = Corpus::open(root).unwrap();
    let log = corpus.request_log().expect("log opens");
    assert!(log.read::<Value>(KEY).unwrap().is_none());
    drop(log);
    // A second run changes nothing.
    let again = prepare_workspace_operations(root).expect("idempotent");
    assert_eq!(again["changed"], false);
    assert!(again.get("recreated").is_none());
    // No staging directory is left behind.
    let leftovers: Vec<_> = fs::read_dir(root.join("_data"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, ["orbit-research-operations"]);
}

#[test]
fn recreation_also_rebuilds_a_missing_data_directory() {
    let temp = prepared_fixture();
    let root = temp.path();
    fs::remove_dir_all(root.join("_data")).expect("delete _data");
    let receipt = prepare_workspace_operations(root).expect("recreate with parent");
    assert_eq!(receipt["recreated"], true);
    assert!(root.join(STATE).join(".layout").is_file());
}

#[test]
fn recreation_never_adopts_or_replaces_state_it_did_not_create() {
    for case in ["foreign_directory", "empty_directory", "symlink", "tracked"] {
        let temp = prepared_fixture();
        let root = temp.path();
        let state = root.join(STATE);
        let outside = tempfile::tempdir().expect("outside");
        fs::write(outside.path().join("sentinel"), "keep").unwrap();
        fs::remove_dir_all(&state).unwrap();
        match case {
            "foreign_directory" => {
                fs::create_dir(&state).unwrap();
                fs::write(state.join("someone-elses.json"), "keep").unwrap();
            }
            "empty_directory" => fs::create_dir(&state).unwrap(),
            "symlink" => symlink(outside.path(), &state).unwrap(),
            "tracked" => {
                fs::create_dir(&state).unwrap();
                fs::write(state.join("tracked.json"), "{}").unwrap();
                git(root, &["add", "-f", &format!("{STATE}/tracked.json")]);
                fs::remove_dir_all(&state).unwrap();
            }
            _ => unreachable!(),
        }
        let before = bytes(root);
        let outside_before = bytes(outside.path());
        let error = prepare_workspace_operations(root).expect_err(case);
        assert!(matches!(error, Error::Refused(_)), "{case}: {error:?}");
        assert_eq!(bytes(root), before, "{case} mutated the corpus");
        assert_eq!(
            bytes(outside.path()),
            outside_before,
            "{case} mutated outside"
        );
        if case == "foreign_directory" || case == "empty_directory" {
            let message = error.to_string();
            assert!(message.contains(".layout"), "{case}: {message}");
            assert!(message.contains("not adopted"), "{case}: {message}");
        }
    }
}

#[test]
fn preparation_makes_git_ignore_plugin_scratch_idempotently() {
    let temp = fixture();
    let root = temp.path();
    let exclude = root.join(".git/info/exclude");
    fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    fs::write(&exclude, "# local rules\n*.swp").expect("existing rules without a final newline");
    legacy(root);

    let first = prepare_workspace_operations(root).expect("prepare");
    assert_eq!(first["scratch_ignored"], true);
    assert_eq!(
        fs::read_to_string(&exclude).unwrap(),
        "# local rules\n*.swp\n/.orbit-research-tmp/\n"
    );
    fs::create_dir(root.join(".orbit-research-tmp")).unwrap();
    fs::write(
        root.join(".orbit-research-tmp/research-acceptance-1.json"),
        "{}",
    )
    .unwrap();
    assert_eq!(git(root, &["status", "--porcelain"]), "");

    let second = prepare_workspace_operations(root).expect("repeat");
    assert_eq!(second["changed"], false);
    assert!(second.get("scratch_ignored").is_none());
    assert_eq!(
        fs::read_to_string(&exclude)
            .unwrap()
            .matches(".orbit-research-tmp")
            .count(),
        1
    );
}

#[test]
fn preparation_leaves_a_corpus_that_already_ignores_scratch_alone() {
    let temp = fixture();
    let root = temp.path();
    fs::write(
        root.join(".gitignore"),
        "_data/**\n!_data/**/\n!_data/**/manifest.json\n/.orbit-research-tmp/\n",
    )
    .unwrap();
    git(root, &["commit", "-qam", "ignore scratch"]);
    legacy(root);
    let exclude = root.join(".git/info/exclude");
    let before = fs::read(&exclude).ok();
    let receipt = prepare_workspace_operations(root).expect("prepare");
    assert!(receipt.get("scratch_ignored").is_none());
    assert_eq!(fs::read(&exclude).ok(), before);
}
