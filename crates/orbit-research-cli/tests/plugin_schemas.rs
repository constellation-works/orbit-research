//! `.orbit-plugin/plugin.yaml`'s `list`/`show`/`check` tools must advertise exactly the
//! operation registry's own derived schema, per ARCHITECTURE.md's operation
//! contract: "Adding a tool requires a typed request, handler and registry
//! entry, rather than a handwritten JSON schema". A schema file that drifts
//! from `Operation::definition()` would let the plugin manifest silently
//! diverge from what Core actually accepts.
use orbit_research_core::application::Operation;
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn registry_schema(operation: Operation) -> Value {
    serde_json::to_value(operation.definition().input_schema).expect("serialize derived schema")
}

fn committed_schema(file: &str, generated: &Value) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".orbit-plugin/schemas")
        .join(file);
    if std::env::var_os("ORBIT_RESEARCH_WRITE_SCHEMAS").is_some() {
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(generated).expect("serialize schema")
            ),
        )
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
    }
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

#[test]
fn list_schema_matches_the_operation_registry() {
    assert_eq!(
        committed_schema("list.request.json", &registry_schema(Operation::List)),
        registry_schema(Operation::List),
        ".orbit-plugin/schemas/list.request.json has drifted from Operation::List; regenerate it from \
         `Operation::List.definition().input_schema`"
    );
}

#[test]
fn check_schema_matches_the_operation_registry() {
    assert_eq!(
        committed_schema("check.request.json", &registry_schema(Operation::Check)),
        registry_schema(Operation::Check),
        ".orbit-plugin/schemas/check.request.json has drifted from Operation::Check; regenerate it from \
         `Operation::Check.definition().input_schema`"
    );
}

#[test]
fn show_schema_matches_the_operation_registry() {
    assert_eq!(
        committed_schema("show.request.json", &registry_schema(Operation::Show)),
        registry_schema(Operation::Show),
        ".orbit-plugin/schemas/show.request.json has drifted from Operation::Show; regenerate it from \
         `Operation::Show.definition().input_schema`"
    );
}

#[test]
fn plan_schema_matches_the_operation_registry() {
    assert_eq!(
        committed_schema("plan.request.json", &registry_schema(Operation::Plan)),
        registry_schema(Operation::Plan),
        ".orbit-plugin/schemas/plan.request.json has drifted from Operation::Plan; regenerate it from \
         `Operation::Plan.definition().input_schema`"
    );
}

/// `version` reports plugin/protocol versions rather than corpus data, so it
/// has no `Operation` of its own (its handler must not require an openable
/// corpus, see `src/plugin.rs`). Its contract is still "no input", so it is
/// pinned to the same empty-object shape `list`/`check` derive from `Empty`,
/// rather than a hand-invented schema.
#[test]
fn version_schema_matches_the_shared_empty_input_shape() {
    assert_eq!(
        committed_schema("version.request.json", &registry_schema(Operation::List)),
        registry_schema(Operation::List),
        ".orbit-plugin/schemas/version.request.json must match the registry's `Empty` input shape"
    );
}

/// The four panel tools take no input, like `version`: each pins to the same
/// empty-object shape the registry derives from `Empty`, never a hand-written
/// schema. Their verbs are not registry operations because each needs a view
/// shaped for the dashboard renderer (see `src/panels.rs`).
#[test]
fn panel_tools_schema_matches_the_shared_empty_input_shape() {
    let empty = registry_schema(Operation::List);
    for file in [
        "open-questions.request.json",
        "awaiting-acceptance.request.json",
        "hypotheses.request.json",
        "corpus-health.request.json",
    ] {
        assert_eq!(
            committed_schema(file, &empty),
            empty,
            ".orbit-plugin/schemas/{file} must match the registry's `Empty` input shape"
        );
    }
}

#[test]
fn manifest_paths_stay_inside_the_plugin_root() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.orbit-plugin");
    let manifest: Value =
        serde_yaml::from_str(&fs::read_to_string(root.join("plugin.yaml")).expect("read manifest"))
            .expect("parse manifest");
    assert!(
        !root.join("../plugin.yaml").exists(),
        "root manifest must be removed"
    );

    let mut paths = vec![
        manifest["spec"]["backend"]["command"]
            .as_str()
            .expect("backend.command"),
    ];
    for tool in manifest["spec"]["tools"].as_array().expect("tools") {
        paths.push(
            tool["input_schema"]["$ref"]
                .as_str()
                .expect("input_schema.$ref"),
        );
    }
    for skill in manifest["spec"]["skills"].as_array().expect("skills") {
        let skill = skill.as_str().expect("skill directory");
        assert!(
            !skill.starts_with('/') && !skill.split('/').any(|part| part == ".."),
            "skill escapes plugin root: {skill}"
        );
        assert!(
            root.join(skill).join("SKILL.md").is_file(),
            "skill directory lacks SKILL.md: {skill}"
        );
    }
    for field in ["activities", "jobs"] {
        for pattern in manifest["spec"]["definitions"][field]
            .as_array()
            .expect("definitions")
        {
            paths.push(pattern.as_str().expect("definition glob"));
        }
    }
    for pattern in manifest["spec"]["tests"].as_array().expect("tests") {
        paths.push(pattern.as_str().expect("test glob"));
    }
    for path in paths {
        assert!(
            !path.starts_with('/') && !path.split('/').any(|part| part == ".."),
            "path escapes plugin root: {path}"
        );
        if let Some(directory) = path.strip_suffix("/*.yaml") {
            assert!(
                fs::read_dir(root.join(directory))
                    .expect("manifest directory")
                    .next()
                    .is_some(),
                "empty manifest glob: {path}"
            );
        } else {
            assert!(root.join(path).is_file(), "missing manifest path: {path}");
        }
    }
    fn assert_no_links(path: &Path) {
        for entry in fs::read_dir(path).expect("plugin directory") {
            let entry = entry.expect("plugin entry");
            let kind = entry.file_type().expect("plugin entry type");
            assert!(
                !kind.is_symlink(),
                "plugin symlink: {}",
                entry.path().display()
            );
            if kind.is_dir() {
                assert_no_links(&entry.path());
            }
        }
    }
    assert_no_links(&root);
}

#[test]
fn bundled_backend_runs_from_an_isolated_plugin_root() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.orbit-plugin");
    let temp = tempfile::tempdir().expect("temporary install");
    let installed = temp.path().join(".orbit-plugin");
    fn copy_plugin(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).expect("create plugin directory");
        for entry in fs::read_dir(source).expect("read plugin directory") {
            let entry = entry.expect("plugin entry");
            if entry.file_name() == "orbit-research.bin" {
                continue;
            }
            let target = destination.join(entry.file_name());
            if entry.file_type().expect("plugin entry type").is_dir() {
                copy_plugin(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).expect("copy plugin file");
            }
        }
    }
    copy_plugin(&source, &installed);
    let bin = installed.join("bin");
    let launcher = bin.join("orbit-research");
    fs::copy(
        env!("CARGO_BIN_EXE_orbit-research"),
        bin.join("orbit-research.bin"),
    )
    .expect("bundle backend binary");
    let mut child = Command::new(&launcher)
        .arg("orbit-tool")
        .current_dir(temp.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("run isolated launcher");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"{\"schema_version\":1,\"tool\":\"version\",\"input\":{}}")
        .expect("write plugin request");
    let output = child.wait_with_output().expect("read plugin response");
    assert!(
        output.status.success(),
        "launcher failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reply: Value = serde_json::from_slice(&output.stdout).expect("plugin JSON reply");
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["output"]["core_version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn launcher_failure_is_valid_json_for_control_characters_in_the_install_path() {
    let temp = tempfile::tempdir().expect("temporary plugin root");
    let controls: String = (1..=31).map(char::from).collect();
    let bin = temp
        .path()
        .join(format!("missing-{controls}-\"\\κ"))
        .join("bin");
    fs::create_dir_all(&bin).expect("isolated launcher directory");
    let launcher = bin.join("orbit-research");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.orbit-plugin/bin/orbit-research"),
        &launcher,
    )
    .expect("copy real launcher");
    let output = Command::new(&launcher)
        .arg("orbit-tool")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("run launcher without a bundled binary");
    assert!(output.status.success(), "structured refusal: {output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let response: Value = serde_json::from_slice(&output.stdout).expect("one JSON plugin response");
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "incompatible_binary");
    assert_eq!(response["error"]["retryable"], false);
    assert_eq!(
        response["error"]["detail"]["path"],
        bin.canonicalize()
            .expect("physical fixture path")
            .join("orbit-research.bin")
            .to_string_lossy()
            .as_ref()
    );
}
