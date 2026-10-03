use crate::{Error, Result, corpus::Corpus};
use serde_json::{Value, json};
use std::{fs, path::Path};

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
        "# Research workspace\n\nQuestions, hypotheses, theories and research items are canonical Markdown. Project names are tags. Use `orbit-research research create` to reserve and commit IDs before starting work. Contributing tasks own `research/R###-slug/code/<unit>/` and `artifacts/<unit>/`; synthesis updates the shared README and input manifest. Orbit owns tasks and execution.\n\nValidate with `orbit-research research check --corpus .` for a concise summary of the base revision and record/tag counts. Use `orbit-research research list --corpus .` to browse records.\n",
    )?;
    fs::write(
        path.join(".gitignore"),
        ".DS_Store\n__pycache__/\n.venv/\n.env\n\n# Research bytes are local; manifests are the committed evidence surface.\n**/data/*\n!**/data/manifest.json\n**/output/**\n_data/**\n!_data/**/\n!_data/**/manifest.json\n\n# Scratch the plugin's accept tool stages artifacts in.\n/.orbit-research-tmp/\n",
    )?;
    fs::write(
        path.join("_scripts/check.sh"),
        "#!/bin/sh\nset -eu\n# Validate and print the base revision and record/tag counts.\nexec orbit-research research check --corpus \"$(CDPATH= cd -- \"$(dirname -- \"$0\")/..\" && pwd)\"\n",
    )?;
    make_check_executable(path)?;
    // The job's `base_branch` defaults to `main`. `symbolic-ref` names the unborn
    // branch on every Git version, unlike `init -b` (Git 2.28+), and ignores
    // `init.defaultBranch`.
    if let Err(error) = run_git(path, &["init", "-q"])
        .and_then(|()| run_git(path, &["symbolic-ref", "HEAD", "refs/heads/main"]))
        .and_then(|()| prepare_new_operations(path))
        .and_then(|()| commit_scaffold(path))
    {
        cleanup_scaffold(path);
        return Err(error);
    }
    let corpus = Corpus::open(path)?;
    Ok(json!({
        "corpus": corpus.root(),
        "created": true,
        "records": 0,
        "revision": corpus.snapshot()?.revision,
    }))
}

fn prepare_new_operations(path: &Path) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    crate::request_log::prepare_workspace_operations(path)?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = path;
    Ok(())
}

fn has_head(path: &Path) -> Result<bool> {
    crate::git::read::has_head(path)
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
    let output = crate::git::command(path).args(args).output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(Error::Internal(format!(
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
