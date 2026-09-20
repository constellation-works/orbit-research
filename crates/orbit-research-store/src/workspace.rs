use crate::{Error, Result, corpus::Corpus};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

/// Existing corpora are validated, never rewritten. New corpora are plain Git repos.
pub fn init(path: &Path) -> Result<Value> {
    if path.join("_scripts/schema.json").exists() {
        let corpus = Corpus::open(path)?;
        let snapshot = corpus.snapshot()?;
        return Ok(
            json!({"corpus":corpus.root(),"created":false,"records":snapshot.records.len(),"revision":snapshot.revision}),
        );
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
        include_bytes!("../resources/schema.json"),
    )?;
    for directory in ["questions", "hypotheses", "theories", "research"] {
        fs::write(path.join(directory).join(".gitkeep"), "")?;
    }
    fs::write(
        path.join("README.md"),
        "# Research workspace\n\nQuestions, hypotheses, theories and research items are canonical Markdown. Project names are tags. Use `orbit-research research create` to reserve and commit IDs before dispatch. Contributing tasks own `research/R###-slug/code/<unit>/` and `artifacts/<unit>/`; synthesis updates the shared README and input manifest. Orbit owns tasks and execution.\n\nValidate with `orbit-research research check --corpus .`. Connect an Orbit backend explicitly when ready; local capture needs no backend.\n",
    )?;
    fs::write(
        path.join(".gitignore"),
        ".DS_Store\n__pycache__/\n.venv/\n.env\n\n# Research bytes are local; manifests are the committed evidence surface.\n**/data/*\n!**/data/manifest.json\n**/output/**\n_data/**\n!_data/**/\n!_data/**/manifest.json\n",
    )?;
    fs::write(
        path.join("_scripts/check.sh"),
        "#!/bin/sh\nset -eu\nexec orbit-research research check --corpus \"$(CDPATH= cd -- \"$(dirname -- \"$0\")/..\" && pwd)\"\n",
    )?;
    for args in [
        vec!["init", "-q"],
        vec![
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
        vec!["commit", "-m", "Initialize research corpus"],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()?;
        if !output.status.success() {
            return Err(Error::Invalid(format!(
                "Scaffold created but Git initialization is incomplete: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
    }
    let corpus = Corpus::open(path)?;
    Ok(json!({"corpus":corpus.root(),"created":true,"revision":corpus.snapshot()?.revision}))
}
