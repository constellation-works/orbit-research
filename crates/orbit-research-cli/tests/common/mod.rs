//! The canonical plugin installed into a private HOME, shared by the ignored
//! tests that need a real Orbit binary (`plugin_v2.rs`, `e2e_v1_loop.rs`).
//!
//! Everything here is private to one test: its own temporary corpus, HOME,
//! Orbit root and plugin export. Nothing touches an existing Orbit workspace or
//! plugin installation.
#![allow(dead_code)]
use serde_json::{Value, json};
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

#[cfg(unix)]
#[path = "../exec_support/mod.rs"]
pub mod exec_support;

pub const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

/// A private corpus, HOME and Orbit workspace with the plugin exported (and,
/// after [`Installed::install`], installed and enabled).
pub struct Installed {
    pub _temp: TempDir,
    pub root: PathBuf,
    pub home: PathBuf,
    pub repository: PathBuf,
    pub source: PathBuf,
    pub orbit: PathBuf,
    pub manifest: Value,
    /// The Orbit workspace name `install` registers for the corpus.
    pub workspace: String,
}

impl Installed {
    /// Export the plugin and initialize an empty corpus from the bundled schema.
    pub fn new(workspace: &str) -> Self {
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
            workspace: workspace.to_owned(),
        };
        let output = fixture
            .research_command()
            .args(["workspace", "init"])
            .arg(&fixture.repository)
            .arg("--json")
            .output()
            .expect("initialize corpus");
        assert!(output.status.success(), "{output:?}");
        fixture
    }

    pub fn command_for(&self, executable: &Path) -> Command {
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
            .env("GIT_AUTHOR_NAME", "Installed plugin fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Installed plugin fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("PATH", std::env::join_paths(paths).expect("fixture PATH"));
        command
    }

    pub fn command(&self) -> Command {
        self.command_for(&self.orbit)
    }

    pub fn research_command(&self) -> Command {
        self.command_for(Path::new(BINARY))
    }

    pub fn git_text(&self, args: &[&str]) -> String {
        let output = self
            .command_for(Path::new("git"))
            .args(args)
            .output()
            .expect("private corpus Git observation");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).expect("Git text")
    }

    /// Bundle the real backend into the export, register the corpus as an Orbit
    /// workspace, install and enable the plugin, and commit Orbit's managed
    /// ignore block so the corpus starts clean.
    pub fn install(&self) {
        let output = self
            .command_for(Path::new("sh"))
            .arg(repository_root().join("scripts/bundle-plugin-binary.sh"))
            .args(["--binary", BINARY])
            .arg(self.source.join(".orbit-plugin"))
            .output()
            .expect("bundle real plugin backend");
        assert!(output.status.success(), "{output:?}");
        for args in [
            vec![
                "workspace",
                "init",
                "--name",
                &self.workspace,
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
        // Orbit initialization adds its managed ignore block. Finalize that
        // fixture-only onboarding change before measuring plugin mutations.
        for args in [
            vec!["add", "--", ".gitignore"],
            vec!["commit", "-m", "Finalize isolated Orbit fixture ignore"],
        ] {
            let output = self
                .command_for(Path::new("git"))
                .args(&args)
                .output()
                .expect("finalize private Orbit onboarding");
            assert!(output.status.success(), "{args:?}: {output:?}");
        }
    }

    pub fn cli_tool(&self, tool: &str, input: &Value) -> Output {
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

pub fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates")
        .parent()
        .expect("repository")
        .to_path_buf()
}

pub fn copy_tree(source: &Path, destination: &Path) {
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

pub fn json_output(output: Output, context: &str) -> Value {
    assert!(output.status.success(), "{context}: {output:?}");
    serde_json::from_slice(&output.stdout).expect("Orbit JSON output")
}

/// MCP structured content must be an object, so Orbit wraps an array output
/// (every table panel) as `{"items": [...]}`; the dashboard reads the bare array.
pub fn mcp_output(result: &Value) -> Value {
    let content = &result["structuredContent"];
    match content.as_object() {
        Some(object) if object.len() == 1 && object.contains_key("items") => {
            content["items"].clone()
        }
        _ => content.clone(),
    }
}

pub fn assert_cli_refusal(output: Output, code: &str) {
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let value: Value = serde_json::from_str(stderr.lines().last().expect("error JSON line"))
        .expect("error object");
    assert_eq!(value["code"], code, "{value}");
}

/// Own and reap the MCP process group, draining both output pipes with byte bounds.
pub struct Mcp {
    child: Child,
    input: ChildStdin,
    responses: Receiver<Value>,
    next_id: u64,
}

impl Mcp {
    pub fn start(fixture: &Installed, operator: bool) -> Self {
        let mut command = fixture.command();
        command.args(["mcp", "serve", "--workspace", &fixture.workspace]);
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

    pub fn send(&mut self, value: Value) {
        writeln!(self.input, "{value}").expect("write MCP request");
        self.input.flush().expect("flush MCP request");
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
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

    pub fn call(&mut self, verb: &str, input: Value) -> Value {
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
