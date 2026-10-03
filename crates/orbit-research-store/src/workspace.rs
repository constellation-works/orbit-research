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
    refuse_unusable_target(path)?;
    let created_root = !path.exists();
    // The job's `base_branch` defaults to `main`. `symbolic-ref` names the unborn
    // branch on every Git version, unlike `init -b` (Git 2.28+), and ignores
    // `init.defaultBranch`.
    let scaffolded = write_scaffold(path)
        .and_then(|()| run_git(path, &["init", "-q"]))
        .and_then(|()| run_git(path, &["symbolic-ref", "HEAD", "refs/heads/main"]))
        .and_then(|()| prepare_new_operations(path))
        .and_then(|()| commit_scaffold(path));
    if let Err(error) = scaffolded {
        cleanup_scaffold(path, created_root);
        return Err(match error {
            Error::Io(error) => Error::Invalid(format!(
                "Cannot create the research corpus at {}: {}. Nothing was created; choose a writable location",
                path.display(),
                plain_io(&error)
            )),
            error => error,
        });
    }
    let corpus = Corpus::open(path)?;
    Ok(json!({
        "corpus": corpus.root(),
        "created": true,
        "records": 0,
        "revision": corpus.snapshot()?.revision,
    }))
}

/// A scaffold target must be a new path under a directory, or an empty
/// directory; anything else is refused with the reason and the next step.
fn refuse_unusable_target(path: &Path) -> Result<()> {
    if path.is_file() {
        return Err(Error::Invalid(format!(
            "{} is a file, not a directory; give a new or empty directory to `orbit-research workspace init`",
            path.display()
        )));
    }
    if !path.exists() {
        // The nearest existing ancestor must be a directory we can create under.
        if let Some(ancestor) = path.ancestors().skip(1).find(|ancestor| ancestor.exists())
            && !ancestor.is_dir()
        {
            return Err(Error::Invalid(format!(
                "Cannot create {}: {} is a file, not a directory; choose a path under a directory",
                path.display(),
                ancestor.display()
            )));
        }
        return Ok(());
    }
    let mut entries = fs::read_dir(path).map_err(|error| {
        Error::Invalid(format!(
            "Cannot read {}: {}",
            path.display(),
            plain_io(&error)
        ))
    })?;
    if entries.next().is_some() {
        return Err(Error::Invalid(format!(
            "{} is not empty and holds no research corpus (it has no _scripts/schema.json), so `workspace init` will not write into it. Choose a new or empty directory, such as `orbit-research workspace init {}`",
            path.display(),
            path.join("observatory").display()
        )));
    }
    Ok(())
}

/// An I/O failure without its trailing `(os error N)`, which tells a reader nothing.
fn plain_io(error: &std::io::Error) -> String {
    let text = error.to_string();
    match text.rfind(" (os error ") {
        Some(end) => text[..end].to_owned(),
        None => text,
    }
}

/// Every scaffold file except the Git repository.
fn write_scaffold(path: &Path) -> Result<()> {
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
    Ok(())
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    if lacks_identity(&stderr) {
        return Err(Error::Invalid(format!(
            "Git has no author identity to commit the scaffold with, so nothing was created. Set one, then rerun `orbit-research workspace init {}`:\n  git config --global user.name \"Your Name\"\n  git config --global user.email you@example.com",
            path.display()
        )));
    }
    Err(Error::Internal(format!(
        "`git {}` failed while creating the research corpus at {}, so nothing was created; fix the cause and rerun `orbit-research workspace init {}`: {}",
        args.join(" "),
        path.display(),
        path.display(),
        stderr.trim()
    )))
}

/// Git's refusal to commit without a name and email, which it explains at length.
fn lacks_identity(stderr: &str) -> bool {
    [
        "Please tell me who you are",
        "Author identity unknown",
        "unable to auto-detect email address",
    ]
    .iter()
    .any(|marker| stderr.contains(marker))
}

/// Remove what a failed scaffold wrote, and the directory itself when this run created it.
fn cleanup_scaffold(path: &Path, created_root: bool) {
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
    if created_root {
        let _ = fs::remove_dir(path);
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
