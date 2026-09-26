use crate::{Error, Result, corpus::Corpus};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Existing corpora are validated, never rewritten. New corpora are plain Git repos.
pub fn init(path: &Path) -> Result<Value> {
    if path.join("_scripts/schema.json").exists() {
        if path.join(".git").exists() && !has_head(path)? {
            return Err(Error::Invalid(format!(
                "Corpus has no commits; inspect and preserve its files, then either commit them explicitly or move them aside before rerunning workspace init at {}",
                path.display()
            )));
        }
        let corpus = Corpus::open(path)?;
        let snapshot = corpus.snapshot()?;
        return Ok(json!({
            "corpus": corpus.root(),
            "created": false,
            "records": snapshot.records.len(),
            "revision": snapshot.revision,
        }));
    }
    if path.exists() && fs::read_dir(path)?.next().is_some() {
        return Err(Error::Invalid(
            "Refusing to scaffold into a nonempty directory without an existing corpus contract"
                .into(),
        ));
    }
    fs::create_dir_all(path)?;
    for directory in [
        "questions",
        "hypotheses",
        "theories",
        "research",
        "_scripts",
        "_data",
    ] {
        fs::create_dir(path.join(directory))?;
    }
    fs::write(
        path.join("_scripts/schema.json"),
        include_bytes!("../assets/schema.json"),
    )?;
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::write(path.join(directory).join(".gitkeep"), "")?;
    }
    fs::write(
        path.join("README.md"),
        "# Research workspace\n\nQuestions, hypotheses, theories and research items are canonical Markdown. Project names are tags. Use `orbit-research research create` to reserve and commit IDs before dispatch. Contributing tasks own `research/R###-slug/code/<unit>/` and `artifacts/<unit>/`; synthesis updates the shared README and input manifest. Orbit owns tasks and execution.\n\nValidate with `orbit-research research check --corpus .` for a concise summary of the base revision and record/tag counts. Use `orbit-research research list --corpus .` to browse records. Connect an Orbit backend explicitly when ready; local capture needs no backend.\n",
    )?;
    fs::write(
        path.join(".gitignore"),
        ".DS_Store\n__pycache__/\n.venv/\n.env\n\n# Research bytes are local; manifests are the committed evidence surface.\n**/data/*\n!**/data/manifest.json\n**/output/**\n_data/**\n!_data/**/\n!_data/**/manifest.json\n",
    )?;
    fs::write(
        path.join("_scripts/check.sh"),
        "#!/bin/sh\nset -eu\n# Validate and print the base revision and record/tag counts.\nexec orbit-research research check --corpus \"$(CDPATH= cd -- \"$(dirname -- \"$0\")/..\" && pwd)\"\n",
    )?;
    make_check_executable(path)?;
    if let Err(error) = run_git(path, &["init", "-q"]).and_then(|()| commit_scaffold(path)) {
        cleanup_scaffold(path);
        return Err(error);
    }
    let corpus = Corpus::open(path)?;
    Ok(json!({
        "corpus": corpus.root(),
        "created": true,
        "revision": corpus.snapshot()?.revision,
    }))
}

fn has_head(path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()?;
    Ok(output.status.success())
}

fn commit_scaffold(path: &Path) -> Result<()> {
    run_git(
        path,
        &[
            "add",
            "--",
            "README.md",
            ".gitignore",
            "_scripts",
            "questions",
            "hypotheses",
            "theories",
            "research",
            "_data",
        ],
    )?;
    run_git(path, &["commit", "-m", "Initialize research corpus"])
}

fn run_git(path: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(Error::Invalid(format!(
        "Scaffold is incomplete and can be resumed with orbit-research workspace init {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn cleanup_scaffold(path: &Path) {
    for entry in [
        ".git",
        ".gitignore",
        "README.md",
        "_data",
        "_scripts",
        "hypotheses",
        "questions",
        "research",
        "theories",
    ] {
        let target = path.join(entry);
        if target.is_dir() {
            let _ = fs::remove_dir_all(target);
        } else {
            let _ = fs::remove_file(target);
        }
    }
}

#[cfg(unix)]
fn make_check_executable(path: &Path) -> Result<()> {
    let script = path.join("_scripts/check.sh");
    let mut permissions = fs::metadata(&script)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(script, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_check_executable(_: &Path) -> Result<()> {
    Ok(())
}
