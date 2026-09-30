#![cfg(unix)]

use std::fs::File;
use std::io::Write;
use std::os::fd::FromRawFd;
use std::process::{Command, Stdio};

const BINARY: &str = env!("CARGO_BIN_EXE_orbit-research");

fn command() -> Command {
    let mut command = Command::new(BINARY);
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Pipe fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Pipe fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid");
    command
}

fn closed_stdout() -> Stdio {
    let mut descriptors = [-1; 2];
    // SAFETY: pipe writes two descriptors to the allocated array on success.
    assert_eq!(unsafe { libc::pipe(descriptors.as_mut_ptr()) }, 0);
    // SAFETY: each successful pipe descriptor is transferred to exactly one File.
    let reader = unsafe { File::from_raw_fd(descriptors[0]) };
    // SAFETY: this is the distinct write descriptor, owned only by this File.
    let writer = unsafe { File::from_raw_fd(descriptors[1]) };
    drop(reader);
    Stdio::from(writer)
}

#[test]
fn closed_stdout_pipes_are_quiet_success_for_cli_and_protocol_commands() {
    let temp = tempfile::tempdir().expect("temporary corpus");
    let corpus = temp.path().join("corpus");
    let output = command()
        .args(["workspace", "init"])
        .arg(&corpus)
        .output()
        .expect("initialize corpus");
    assert!(output.status.success(), "{output:?}");
    for (args, input) in [
        (vec!["--help"], None),
        (vec!["resource"], None),
        (
            vec![
                "mcp",
                "--corpus",
                corpus.to_str().expect("UTF-8 fixture path"),
            ],
            Some(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n".as_slice()),
        ),
        (
            vec!["orbit-tool"],
            Some(b"{\"schema_version\":1,\"tool\":\"version\",\"input\":{}}".as_slice()),
        ),
    ] {
        let mut child = command()
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(closed_stdout())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run pipe fixture");
        let mut stdin = child.stdin.take().expect("child stdin");
        if let Some(input) = input {
            stdin.write_all(input).expect("write protocol request");
        }
        drop(stdin);
        let output = child.wait_with_output().expect("finish pipe fixture");
        assert!(output.status.success(), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
    }
}
