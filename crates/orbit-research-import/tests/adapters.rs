//! Behavioral coverage for all four read-only owner adapters, using the same
//! synthetic source trees as `examples/make_fixture_sources.py`, plus the
//! read-only/dry-run and strict-JSON invariants documented in
//! `docs/imports.md`.

use std::fs;
use std::path::{Path, PathBuf};

use orbit_research_contract::validate;
use orbit_research_import::{import_source, write_report};
use serde_json::Value;

/// Port of `examples/make_fixture_sources.py::create`: four small synthetic
/// owner source trees, one per adapter, under `destination`.
fn create_fixture(destination: &Path) {
    put(
        destination,
        "principia/theory/calibration/claims.json",
        r#"{
  "doc": "calibration",
  "title": "Synthetic control calibration",
  "status": "retired",
  "claims": [
    {
      "id": "control-null",
      "kind": "model-property",
      "status": "mixed",
      "claim": "The synthetic zero-effect control stays below the fixed threshold.",
      "evidence": "Execution completed, but the negative control failed.",
      "control_ran": true,
      "control": "failed",
      "limitation": "No inference about nature."
    }
  ]
}
"#,
    );
    put(
        destination,
        "principia/gates/control.json",
        r#"{
  "id": "control",
  "control": "zero injected effect",
  "kill": "absolute error >= 0.04",
  "decision": "Failed control leaves primary inference unresolved."
}
"#,
    );
    put(
        destination,
        "parallax/docs/register.md",
        "# Synthetic empirical program\n\n| ID | Statement | Comparator |\n| --- | --- | --- |\n| R1 | Market measurement program | None |\n| H1 | Forecast lift survives measured costs. | Constant forecast |\n| E1 | Proposed walk-forward test | No outcome yet |\n",
    );
    create_journal_sqlite(&destination.join("parallax/data/journal.sqlite"));
    put(
        destination,
        "orrery/lab/sims/control/sim.json",
        r#"{
  "slug": "control",
  "title": "Synthetic control",
  "kind": "py",
  "status": "retired"
}
"#,
    );
    put(
        destination,
        "orrery/lab/sims/control/assets/results.json",
        r#"{
  "execution": "completed",
  "decision": {
    "control": "failed",
    "verdict": "inconclusive"
  },
  "realizations": [
    {
      "run_id": "R0",
      "error": 0.05
    }
  ]
}
"#,
    );
    put(
        destination,
        "astrolabe/data/processed/derived/example.json",
        r#"{
  "name": "example",
  "kind": "derived",
  "source": "analysis.example",
  "query": {
    "synthetic": true
  },
  "fetched_at": "2020-01-01T00:00:00Z",
  "n_rows": 0,
  "columns": [
    "x"
  ],
  "lineage": [
    {
      "dataset": "parent",
      "fetched_at": "2019-01-01T00:00:00Z"
    },
    {
      "dataset": null,
      "note": "not persisted"
    }
  ]
}
"#,
    );
}

fn put(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir -p");
    fs::write(path, content).expect("write fixture file");
}

fn create_journal_sqlite(path: &Path) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir -p");
    let conn = rusqlite::Connection::open(path).expect("open sqlite");
    conn.execute_batch(
        "CREATE TABLE trade_intents(id TEXT PRIMARY KEY, created_at TEXT, hypothesis TEXT, entry_rule TEXT, exit_rule TEXT, invalidation TEXT, size TEXT, expected_edge_bps REAL);
         CREATE TABLE trade_outcomes(trade_id TEXT PRIMARY KEY, recorded_at TEXT, fill TEXT, fees TEXT, slippage_bps REAL, result TEXT);",
    )
    .expect("create tables");
    conn.execute(
        "INSERT INTO trade_intents VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            "intent-1",
            "2020-01-01T00:00:00Z",
            "Forecast lift survives costs.",
            "fixed entry",
            "fixed exit",
            "no lift",
            "unit",
            2.0,
        ],
    )
    .expect("insert intent");
    conn.execute(
        "INSERT INTO trade_outcomes VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            "intent-1",
            "2020-01-02T00:00:00Z",
            "one fill",
            "one fee",
            1.0,
            "Negative after costs.",
        ],
    )
    .expect("insert outcome");
}

fn candidate_ids(report: &Value) -> Vec<String> {
    report["candidates"]
        .as_array()
        .expect("candidates array")
        .iter()
        .map(|r| r["id"].as_str().expect("id").to_string())
        .collect()
}

#[test]
fn principia_matches_python_oracle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("principia");
    fs::create_dir(&root).expect("mkdir root");
    put(
        &root,
        "theory/calibration/claims.json",
        r#"{
  "doc": "calibration",
  "title": "Synthetic control calibration",
  "status": "retired",
  "claims": [
    {
      "id": "control-null",
      "kind": "model-property",
      "status": "mixed",
      "claim": "The synthetic zero-effect control stays below the fixed threshold.",
      "evidence": "Execution completed, but the negative control failed.",
      "control_ran": true,
      "control": "failed",
      "limitation": "No inference about nature."
    }
  ]
}
"#,
    );
    put(
        &root,
        "gates/control.json",
        r#"{
  "id": "control",
  "control": "zero injected effect",
  "kill": "absolute error >= 0.04",
  "decision": "Failed control leaves primary inference unresolved."
}
"#,
    );

    let report = import_source(&root, "principia", "principia", None, None).expect("import succeeds");

    let oracle_bytes = fs::read("../../examples/migration-report.json").expect("read oracle fixture");
    let oracle: Value = serde_json::from_slice(&oracle_bytes).expect("oracle parses");

    assert_eq!(
        report, oracle,
        "Rust importer output diverged from the Python oracle on the synthetic principia fixture"
    );
}

#[test]
fn all_four_adapters_cover_the_synthetic_fixture() {
    let tmp = tempfile::tempdir().expect("tempdir");
    create_fixture(tmp.path());

    for adapter in orbit_research_import::ADAPTERS {
        let root = tmp.path().join(adapter);
        let report =
            import_source(&root, adapter, adapter, None, None).unwrap_or_else(|e| panic!("{adapter} import failed: {e}"));

        assert_eq!(report["kind"], "import-report");
        assert_eq!(report["adapter"], *adapter);
        assert_eq!(report["dry_run"], true);
        assert_eq!(report["source_unchanged"], true);

        let errors = validate(&report, &[]);
        assert!(errors.is_empty(), "{adapter} report failed contract validation: {errors:?}");

        let discovered = report["counts"]["discovered"].as_u64().expect("discovered");
        let mapped = report["counts"]["mapped"].as_u64().expect("mapped");
        let exceptions = report["counts"]["exceptions"].as_u64().expect("exceptions");
        assert!(discovered > 0, "{adapter} discovered nothing");
        assert_eq!(discovered, mapped + exceptions);
    }
}

#[test]
fn principia_fixture_candidate_ids() {
    let tmp = tempfile::tempdir().expect("tempdir");
    create_fixture(tmp.path());
    let report = import_source(&tmp.path().join("principia"), "principia", "principia", None, None)
        .expect("import succeeds");
    let mut ids = candidate_ids(&report);
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "urn:research:principia:assessment:control-null%3Alegacy-verdict",
            "urn:research:principia:claim:control-null",
            "urn:research:principia:program:calibration",
            "urn:research:principia:protocol:control",
        ]
    );
}

#[test]
fn parallax_fixture_covers_table_row_and_sqlite_journal() {
    let tmp = tempfile::tempdir().expect("tempdir");
    create_fixture(tmp.path());
    let report = import_source(&tmp.path().join("parallax"), "parallax", "parallax", None, None)
        .expect("import succeeds");
    let ids = candidate_ids(&report);
    assert!(ids.iter().any(|id| id == "urn:research:parallax:claim:H1"));
    assert!(ids.iter().any(|id| id.starts_with("urn:research:parallax:claim:journal%3Aintent-1")));
    assert!(ids.iter().any(|id| id.starts_with("urn:research:parallax:experiment:journal%3Aintent-1")));
    // trade_intents/trade_outcomes are never fabricated into a protocol; only
    // research_intents rows get a paired historical-unverified protocol.
    assert!(!ids.iter().any(|id| id.starts_with("urn:research:parallax:protocol:")));
}

#[test]
fn orrery_fixture_maps_sim_catalog_and_retains_result_artifact() {
    let tmp = tempfile::tempdir().expect("tempdir");
    create_fixture(tmp.path());
    let report = import_source(&tmp.path().join("orrery"), "orrery", "orrery", None, None).expect("import succeeds");
    let ids = candidate_ids(&report);
    assert!(ids.contains(&"urn:research:orrery:program:control".to_string()));
    assert!(ids.iter().any(|id| id.starts_with("urn:research:orrery:artifact:")));
}

#[test]
fn astrolabe_fixture_maps_dataset_and_retains_lineage_members() {
    let tmp = tempfile::tempdir().expect("tempdir");
    create_fixture(tmp.path());
    let report = import_source(&tmp.path().join("astrolabe"), "astrolabe", "astrolabe", None, None)
        .expect("import succeeds");
    let ids = candidate_ids(&report);
    assert!(ids.contains(&"urn:research:astrolabe:artifact:derived%3Aexample".to_string()));
    let inventory = report["inventory"].as_array().expect("inventory");
    assert!(
        inventory
            .iter()
            .any(|item| item["selector"] == "$/lineage/0" && item["disposition"] == "exception")
    );
    assert!(
        inventory
            .iter()
            .any(|item| item["selector"] == "$/lineage/1" && item["disposition"] == "exception")
    );
}

#[test]
fn dry_run_never_writes_and_write_report_refuses_unsafe_destinations() {
    let tmp = tempfile::tempdir().expect("tempdir");
    create_fixture(tmp.path());
    let root = tmp.path().join("principia");

    let before: Vec<(PathBuf, Vec<u8>)> = walk_files(&root)
        .into_iter()
        .map(|p| {
            let bytes = fs::read(&p).expect("read");
            (p, bytes)
        })
        .collect();

    let report = import_source(&root, "principia", "principia", None, None).expect("import succeeds");

    let after = walk_files(&root);
    assert_eq!(
        after.len(),
        before.len(),
        "import must not create or delete files in the source root"
    );
    for (path, bytes) in &before {
        assert_eq!(&fs::read(path).expect("read after"), bytes, "import must not mutate source bytes");
    }

    // Refuses a destination inside the source root.
    let inside = root.join("report.json");
    let err = write_report(&report, &inside, &[root.clone()]).unwrap_err();
    assert!(err.to_string().contains("outside every source root"));
    assert!(!inside.exists());

    // Succeeds outside the source root.
    let outside = tmp.path().join("report.json");
    write_report(&report, &outside, &[root.clone()]).expect("write outside source root");
    assert!(outside.exists());

    // Refuses to overwrite an existing file.
    let err = write_report(&report, &outside, &[root.clone()]).unwrap_err();
    assert!(!err.to_string().is_empty());
}

#[test]
fn duplicate_json_keys_and_non_finite_numbers_are_parse_errors() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("principia");
    fs::create_dir(&root).expect("mkdir root");
    fs::create_dir_all(root.join("gates")).expect("mkdir gates");
    fs::write(root.join("gates/duplicate.json"), r#"{"id":"a","id":"b"}"#).expect("write duplicate-key file");
    fs::write(root.join("gates/nonfinite.json"), r#"{"id":"c","value":NaN}"#).expect("write non-finite file");

    let report = import_source(&root, "principia", "principia", None, None).expect("import succeeds");
    let inventory = report["inventory"].as_array().expect("inventory");

    for path in ["gates/duplicate.json", "gates/nonfinite.json"] {
        let item = inventory
            .iter()
            .find(|item| item["path"] == path)
            .unwrap_or_else(|| panic!("no inventory entry for {path}"));
        assert_eq!(item["disposition"], "exception");
        let codes: Vec<&str> = item["exceptions"]
            .as_array()
            .expect("exceptions")
            .iter()
            .map(|e| e["code"].as_str().expect("code"))
            .collect();
        assert_eq!(codes, vec!["read-error"], "{path} should fail to parse, not silently coerce");
    }
}

fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_files_into(root, &mut out);
    out.sort();
    out
}

fn walk_files_into(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            walk_files_into(&path, out);
        } else {
            out.push(path);
        }
    }
}
