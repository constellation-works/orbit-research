//! The maintenance scripts answer `--help` with usage and a zero exit, and an
//! unknown argument is a usage error that changes nothing. PATH holds only the
//! system directories, so a script that went on to run `cargo` (or the real
//! `orbit-research`) would fail here instead of rewriting the checkout.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(script: &str, args: &[&str]) -> Output {
    Command::new("/bin/sh")
        .arg(repository_root().join("scripts").join(script))
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("run the script")
}

/// Every file under the plugin tree with its bytes.
fn plugin_tree() -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(directory: &Path, files: &mut Vec<(PathBuf, Vec<u8>)>) {
        let mut entries: Vec<_> = fs::read_dir(directory)
            .expect("plugin directory")
            .map(|entry| entry.expect("entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, files);
            } else {
                let bytes = fs::read(&path).expect("plugin file");
                files.push((path, bytes));
            }
        }
    }
    let mut files = Vec::new();
    walk(&repository_root().join(".orbit-plugin"), &mut files);
    files
}

#[test]
fn schema_generator_prints_usage_and_rejects_unknown_arguments_without_writing() {
    let before = plugin_tree();
    for flag in ["--help", "-h"] {
        let output = run("generate-plugin-schemas.sh", &[flag]);
        assert_eq!(output.status.code(), Some(0), "{flag}: {output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.starts_with("usage: scripts/generate-plugin-schemas.sh"),
            "{stdout}"
        );
    }
    for args in [&["--bogus"][..], &["extra"][..], &["--help", "extra"][..]] {
        let output = run("generate-plugin-schemas.sh", args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("usage:"),
            "{args:?}: {output:?}"
        );
    }
    assert!(
        plugin_tree() == before,
        "the generator must not write on a usage error"
    );
}

#[test]
fn binary_bundler_prints_usage_with_a_zero_exit_and_still_rejects_bad_usage() {
    let before = plugin_tree();
    for flag in ["--help", "-h"] {
        let output = run("bundle-plugin-binary.sh", &[flag]);
        assert_eq!(output.status.code(), Some(0), "{flag}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("usage: "),
            "{flag}: {output:?}"
        );
    }
    for args in [&["--nope"][..], &["--binary"][..], &["one", "two"][..]] {
        let output = run("bundle-plugin-binary.sh", args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
    }
    assert!(
        plugin_tree() == before,
        "the bundler must not write on a usage error"
    );
}

/// Runs the bash script through its own shebang, as `make` does, with `path`
/// as the whole PATH.
fn run_bash_script(script: &str, args: &[&str], path: &str) -> Output {
    Command::new(repository_root().join("scripts").join(script))
        .args(args)
        .env_clear()
        .env("PATH", path)
        .output()
        .expect("run the script")
}

#[test]
fn dependency_checker_prints_usage_rejects_unknown_arguments_and_names_a_missing_cargo() {
    for flag in ["--help", "-h"] {
        let output = run_bash_script("check-dependency-direction.sh", &[flag], "/usr/bin:/bin");
        assert_eq!(output.status.code(), Some(0), "{flag}: {output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.starts_with("usage: scripts/check-dependency-direction.sh"),
            "{stdout}"
        );
    }
    for args in [&["--bogus"][..], &["--self-test", "extra"][..], &["x"][..]] {
        let output = run_bash_script("check-dependency-direction.sh", args, "/usr/bin:/bin");
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unknown argument") && stderr.contains("usage:"),
            "{args:?}: {stderr}"
        );
    }
    // A PATH holding only bash: no cargo, so the script must say so plainly
    // rather than fail inside `cargo metadata`.
    let bin = tempfile::tempdir().expect("private PATH");
    let bash = ["/bin/bash", "/usr/bin/bash"]
        .into_iter()
        .find(|path| Path::new(path).exists())
        .expect("bash");
    std::os::unix::fs::symlink(bash, bin.path().join("bash")).expect("bash link");
    let output = run_bash_script(
        "check-dependency-direction.sh",
        &["--self-test"],
        bin.path().to_str().expect("UTF-8 path"),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("`cargo` was not found on PATH"), "{stderr}");
}
