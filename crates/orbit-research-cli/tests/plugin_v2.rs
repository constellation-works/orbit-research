//! The canonical plugin installed into a private HOME, exercised through real Orbit.
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[test]
#[ignore = "requires ORBIT_RESEARCH_TEST_ORBIT_BIN and its native plugin sandbox"]
fn installed_plugin_serves_every_tool_over_cli_and_mcp() {
    let fixture = Fixture::new();
    fixture.install();
    let advertised = fixture.manifest["spec"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<BTreeSet<_>>();
    let reads = read_requests();
    let exercised = reads
        .iter()
        .map(|(tool, _)| *tool)
        .chain(["link", "validate", "accept"])
        .collect::<BTreeSet<_>>();
    assert_eq!(advertised, exercised, "exercise every advertised tool");
    for (tool, input) in &reads {
        let value = json_output(fixture.cli_tool(tool, input), tool);
        assert_read(tool, &value);
    }
    let link = json!({"research_id":"R001", "request_key":"cli-link", "title":"Investigate R001"});
    let created = json_output(fixture.cli_tool("link", &link), "link");
    assert_eq!(created["created"], true, "{created}");
    let task = created["task_id"].as_str().expect("created task id");
    let adopted = json_output(fixture.cli_tool("link", &link), "link retry");
    assert_eq!(adopted["created"], false, "{adopted}");
    assert_eq!(adopted["task_id"], task, "retry adopts the exact task");
    let validate = json!({"path": fixture.repository.to_string_lossy()});
    assert_cli_refusal(
        fixture.cli_tool("validate", &validate),
        "run_context_required",
    );
    let accept = json!({"task_id":task, "research_id":"R001"});
    assert_cli_refusal(fixture.cli_tool("accept", &accept), "refused");

    let mut mcp = Mcp::start(&fixture, true);
    let listed = mcp.request("tools/list", json!({}));
    let names = listed["tools"]
        .as_array()
        .expect("MCP tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .filter_map(|name| name.strip_prefix("research_"))
        .collect::<BTreeSet<_>>();
    assert_eq!(names, advertised, "the MCP surface lists every plugin tool");
    for (tool, input) in reads {
        let result = mcp.call(tool, input);
        assert_ne!(result["isError"], true, "{tool}: {result}");
        assert_read(tool, &result["structuredContent"]);
    }
    let link = json!({"research_id":"R001", "request_key":"mcp-link", "title":"Investigate R001"});
    let created = mcp.call("link", link.clone());
    assert_ne!(created["isError"], true, "{created}");
    assert_eq!(created["structuredContent"]["created"], true, "{created}");
    let adopted = mcp.call("link", link);
    assert_ne!(adopted["isError"], true, "{adopted}");
    assert_eq!(adopted["structuredContent"]["created"], false, "{adopted}");
    assert_eq!(
        adopted["structuredContent"]["task_id"],
        created["structuredContent"]["task_id"]
    );
    for (tool, input, code) in [
        ("validate", validate, "run_context_required"),
        ("accept", accept, "refused"),
    ] {
        let result = mcp.call(tool, input);
        assert_eq!(result["isError"], true, "{tool}: {result}");
        assert_eq!(result["structuredContent"]["code"], code, "{result}");
    }
    let invalid = mcp.call("version", json!({"unknown_field":true}));
    assert_eq!(invalid["isError"], true, "{invalid}");
    assert!(
        invalid["structuredContent"]
            .to_string()
            .contains("unknown_field"),
        "{invalid}"
    );
}

fn read_requests() -> Vec<(&'static str, Value)> {
    vec![
        ("version", json!({})),
        ("list", json!({})),
        ("show", json!({"id":"R001"})),
        ("check", json!({})),
        (
            "plan",
            json!({"shape":"investigation", "research_id":"R001", "objective":"Reproduce the baseline"}),
        ),
    ]
}

fn assert_read(tool: &str, value: &Value) {
    match tool {
        "version" => assert_eq!(
            value["core_version"],
            orbit_research_core::VERSION,
            "{value}"
        ),
        "list" => assert_eq!(value["records"][0]["id"], "R001", "{value}"),
        "show" => assert_eq!(value["id"], "R001", "{value}"),
        "check" => {
            assert_eq!(value["valid"], true, "{value}");
            assert_eq!(value["record_count"], 1, "{value}");
        }
        "plan" => assert_eq!(
            value["context_files"],
            json!(["dir:research/R001-study"]),
            "{value}"
        ),
        _ => panic!("unexpected read tool: {tool}"),
    }
}

fn json_output(output: Output, context: &str) -> Value {
    assert!(output.status.success(), "{context}: {output:?}");
    serde_json::from_slice(&output.stdout).expect("Orbit JSON output")
}

fn assert_cli_refusal(output: Output, code: &str) {
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let value: Value = serde_json::from_str(stderr.lines().last().expect("error JSON line"))
        .expect("error object");
    assert_eq!(value["code"], code, "{value}");
}

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
    home: PathBuf,
    repository: PathBuf,
    source: PathBuf,
    orbit: PathBuf,
    manifest: Value,
}

impl Fixture {
    fn new() -> Self {
        let orbit = std::env::var_os("ORBIT_RESEARCH_TEST_ORBIT_BIN")
            .filter(|value| !value.is_empty())
            .expect("set ORBIT_RESEARCH_TEST_ORBIT_BIN to run installed-plugin QA");
        let orbit = Path::new(&orbit)
            .canonicalize()
            .expect("Orbit executable path");
        let temp = TempDir::new().expect("private plugin fixture");
        let root = temp.path().canonicalize().expect("physical fixture root");
        let home = root.join("home");
        let repository = root.join("corpus");
        let source = root.join("source");
        for path in [&home, &repository, &source] {
            fs::create_dir(path).expect("fixture directory");
        }
        copy_tree(
            &repository_root().join(".orbit-plugin"),
            &source.join(".orbit-plugin"),
        );
        let manifest_path = source.join(".orbit-plugin/plugin.yaml");
        let text = fs::read_to_string(&manifest_path).expect("manifest");
        // Local directory exports have no verified first-party origin.
        let text = text.replace("  origin: orbit\n", "");
        fs::write(&manifest_path, &text).expect("local manifest");
        let manifest = serde_yaml::from_str(&text).expect("manifest YAML");
        let fixture = Self {
            _temp: temp,
            root,
            home,
            repository,
            source,
            orbit,
            manifest,
        };
        let output = fixture
            .research_command()
            .args(["workspace", "init"])
            .arg(&fixture.repository)
            .arg("--json")
            .output()
            .expect("initialize corpus");
        assert!(output.status.success(), "{output:?}");
        let output = fixture
            .research_command()
            .args(["research", "create", "--corpus"])
            .arg(&fixture.repository)
            .args([
                "--kind",
                "R",
                "--status",
                "planned",
                "--title",
                "Study",
                "--body",
                "Question under study",
                "--request-key",
                "reserve",
                "--json",
            ])
            .output()
            .expect("reserve a research record");
        assert!(output.status.success(), "{output:?}");
        fixture
    }

    fn command_for(&self, executable: &Path) -> Command {
        let mut command = Command::new(executable);
        let mut paths = vec![
            self.orbit
                .parent()
                .expect("Orbit bin directory")
                .to_path_buf(),
        ];
        // Preserve executable discovery, but all configuration and identity is private.
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        command
            .env_clear()
            .current_dir(&self.repository)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.home)
            .env("TMPDIR", &self.root)
            .env("ORBIT_BIN", &self.orbit)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("PATH", std::env::join_paths(paths).expect("fixture PATH"));
        command
    }

    fn command(&self) -> Command {
        self.command_for(&self.orbit)
    }
    fn research_command(&self) -> Command {
        self.command_for(Path::new(env!("CARGO_BIN_EXE_orbit-research")))
    }

    fn install(&self) {
        let output = self
            .command_for(Path::new("sh"))
            .arg(repository_root().join("scripts/bundle-plugin-binary.sh"))
            .args(["--binary", env!("CARGO_BIN_EXE_orbit-research")])
            .arg(self.source.join(".orbit-plugin"))
            .output()
            .expect("bundle real plugin backend");
        assert!(output.status.success(), "{output:?}");
        for args in [
            vec![
                "workspace",
                "init",
                "--name",
                "research-v2-test",
                "--ship-mode",
                "local",
            ],
            vec![
                "plugin",
                "add",
                self.source
                    .join(".orbit-plugin")
                    .to_str()
                    .expect("plugin path"),
            ],
            vec!["plugin", "enable", "research", "--grant", "fs,orbit_tools"],
        ] {
            let output = self
                .command()
                .args(&args)
                .output()
                .expect("install isolated plugin");
            assert!(output.status.success(), "{args:?}: {output:?}");
        }
    }

    fn cli_tool(&self, tool: &str, input: &Value) -> Output {
        // A fully cleared shell has no caller identity. Use Orbit's explicit
        // audited operator override for CLI QA.
        self.command()
            .env("ORBIT_OPERATOR", "1")
            .args([
                "tool",
                "run",
                &format!("research.{tool}"),
                "--input",
                &input.to_string(),
                "--full",
            ])
            .output()
            .expect("invoke installed plugin tool")
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates")
        .parent()
        .expect("repository")
        .to_path_buf()
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir(destination).expect("plugin directory");
    for entry in fs::read_dir(source).expect("plugin entries") {
        let entry = entry.expect("plugin entry");
        let kind = entry.file_type().expect("plugin entry type");
        assert!(!kind.is_symlink(), "plugin tree contains no links");
        if entry.file_name() == "orbit-research.bin" {
            continue;
        }
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("plugin file");
        }
    }
}

/// Own and reap the MCP process group, draining both output pipes with byte bounds.
struct Mcp {
    child: Child,
    input: ChildStdin,
    responses: Receiver<Value>,
    next_id: u64,
}

impl Mcp {
    fn start(fixture: &Fixture, operator: bool) -> Self {
        let mut command = fixture.command();
        command.args(["mcp", "serve", "--workspace", "research-v2-test"]);
        if operator {
            command.arg("--operator");
        }
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start real Orbit MCP server");
        let input = child.stdin.take().expect("MCP stdin");
        let stdout = child.stdout.take().expect("MCP stdout");
        let stderr = child.stderr.take().expect("MCP stderr");
        let (sender, responses) = mpsc::sync_channel(4);
        thread::spawn(move || {
            for line in BufReader::new(stdout.take(4 * 1024 * 1024)).lines() {
                let line = line.expect("MCP stdout line");
                let response = serde_json::from_str(&line).expect("MCP JSON line");
                if sender.send(response).is_err() {
                    break;
                }
            }
        });
        thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr
                .take(1024 * 1024)
                .read_to_end(&mut bytes)
                .expect("drain MCP stderr");
        });
        let mut session = Self {
            child,
            input,
            responses,
            next_id: 1,
        };
        let initialized = session.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18",
            "capabilities": {}, "clientInfo": {"name": "research-v2-test", "version": "1"}}),
        );
        assert!(
            initialized["capabilities"]["tools"].is_object(),
            "{initialized}"
        );
        session.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        session
    }

    fn send(&mut self, value: Value) {
        writeln!(self.input, "{value}").expect("write MCP request");
        self.input.flush().expect("flush MCP request");
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let value = self
                .responses
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("MCP response before deadline");
            if value["id"] == id {
                assert!(value.get("error").is_none(), "{method}: {value}");
                return value["result"].clone();
            }
        }
    }

    fn call(&mut self, verb: &str, input: Value) -> Value {
        self.request(
            "tools/call",
            json!({"name": format!("research_{verb}"), "arguments": input}),
        )
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Ok(group) = libc::pid_t::try_from(self.child.id()) {
            // SAFETY: this child has not been reaped, so the process-group
            // identity created at spawn cannot have been reused.
            unsafe {
                libc::killpg(group, libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
