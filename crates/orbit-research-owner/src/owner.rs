//! Owner-native immutable JSON appends.
//!
//! Git supplies publication pins, Orbit supplies execution provenance. This module authors
//! JSON under `research/`; it never commits, pushes, or talks to an Orbit store.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use orbit_research_contract::{
    canonical_json, parse_json, protocol_digest, reconcile, record_references, revision_digest,
    validate, validate_structural,
};
use serde_json::{Value, json};

use crate::artifacts::{ArtifactSource, digest_bytes};
use crate::checkout::Checkout;
use crate::git::{Git, SubprocessGit};
use crate::native::{
    RecordKey, confirmation_errors, native_errors, native_reference_errors, record_key,
    reference_key,
};
use crate::time::{Clock, SystemClock, instant};
use crate::{OwnerError, Result, require};

/// Authoring operations and the record kind each one appends. `retire` takes its kind from
/// the request because retirement preserves an existing program or claim revision.
pub const OPERATIONS: &[(&str, Option<&str>)] = &[
    ("program", Some("program")),
    ("claim", Some("claim")),
    ("artifact", Some("artifact")),
    ("preregister", Some("protocol")),
    ("begin-run", Some("experiment")),
    ("record-run", Some("experiment")),
    ("assess", Some("assessment")),
    ("retire", None),
];

const KINDS: &[&str] = &[
    "program",
    "claim",
    "artifact",
    "protocol",
    "experiment",
    "assessment",
];

const REQUEST_FIELDS: &[&str] = &[
    "request_id",
    "id",
    "scope",
    "payload",
    "orbit_links",
    "expected_heads",
    "supersedes",
    "reason",
    "references",
    "presentation",
    "limitations",
    "kind",
];

/// Explicit owner wiring. Nothing here is inferred from the current directory.
#[derive(Clone)]
pub struct OwnerConfig {
    /// Canonical records directory, relative to the owner root and under `research/`.
    pub records: String,
    /// Additional routed source checkouts, keyed by repository namespace.
    pub sources: BTreeMap<String, PathBuf>,
    pub git: Arc<dyn Git>,
    pub clock: Arc<dyn Clock>,
    pub artifact_resolver: Option<Arc<dyn ArtifactSource>>,
}

impl Default for OwnerConfig {
    fn default() -> Self {
        Self {
            records: "research/records".to_owned(),
            sources: BTreeMap::new(),
            git: Arc::new(SubprocessGit),
            clock: Arc::new(SystemClock),
            artifact_resolver: None,
        }
    }
}

pub struct Owner {
    checkout: Checkout,
    repository: String,
    relative: String,
    directory: PathBuf,
    sources: BTreeMap<String, Checkout>,
    clock: Arc<dyn Clock>,
    artifact_resolver: Option<Arc<dyn ArtifactSource>>,
}

impl Owner {
    /// Open an explicit owner checkout and its routed sources.
    pub fn open(root: &Path, repository: &str, config: OwnerConfig) -> Result<Self> {
        let mut characters = repository.chars();
        require(
            characters
                .next()
                .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
                && characters.all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || character == '.'
                        || character == '-'
                }),
            "explicit repository namespace required",
        )?;
        let checkout = Checkout::open(root, config.git.clone())?;
        let relative = Path::new(&config.records)
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/");
        require(
            relative.starts_with("research/"),
            "canonical records must live under research/ for exact source discovery",
        )?;
        let directory = checkout.safe_path(&config.records)?;
        let mut sources = BTreeMap::new();
        sources.insert(repository.to_owned(), checkout.clone());
        for (name, path) in &config.sources {
            require(name != repository, "cannot override owner routing")?;
            sources.insert(name.clone(), Checkout::open(path, config.git.clone())?);
        }
        Ok(Self {
            checkout,
            repository: repository.to_owned(),
            relative,
            directory,
            sources,
            clock: config.clock,
            artifact_resolver: config.artifact_resolver,
        })
    }

    /// The canonical records directory. Exports and reports may never be written inside it.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn checkout(&self) -> &Checkout {
        &self.checkout
    }

    /// Lock the directory inode: no mutable database, index or persistent lock file.
    fn locked(&self) -> Result<DirectoryLock> {
        std::fs::create_dir_all(&self.directory)?;
        DirectoryLock::acquire(&self.directory)
    }

    /// Every canonical append, verified as a complete unforked chain.
    pub fn entries(&self) -> Result<Vec<(PathBuf, Value)>> {
        let committed = self
            .checkout
            .text(&["ls-tree", "-r", "--name-only", "HEAD", "--", &self.relative])
            .unwrap_or_default();
        for relative in committed.lines() {
            let name = Path::new(relative)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if is_append_name(name) {
                require(
                    self.checkout.safe_path(relative)?.is_file(),
                    "committed append was removed; restore owner history before writing",
                )?;
            }
        }
        let mut paths: Vec<PathBuf> = match std::fs::read_dir(&self.directory) {
            Ok(listing) => listing
                .filter_map(std::result::Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|suffix| suffix == "json"))
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        paths.sort();
        let mut rows: Vec<(PathBuf, Value)> = Vec::new();
        let mut previous: Option<String> = None;
        for path in paths {
            require(
                !path
                    .symlink_metadata()
                    .is_ok_and(|data| data.file_type().is_symlink()),
                "canonical record cannot be a symlink",
            )?;
            let record = parse_json(&std::fs::read(&path)?)?;
            require(record.is_object(), "invalid canonical record")?;
            let errors = validate_structural(&record);
            require(
                errors.is_empty(),
                &format!("invalid canonical record: {}", errors.join("; ")),
            )?;
            require(
                record.get("schema_version").and_then(Value::as_u64) == Some(2)
                    && provenance(&record, "repository") == Some(self.repository.as_str()),
                "wrong native schema/owner",
            )?;
            let authorship = record.get("authorship").cloned().unwrap_or(Value::Null);
            let digest = digest_bytes(&canonical_json(&record)?);
            let sequence = rows.len() + 1;
            require(
                path.file_name().and_then(|name| name.to_str())
                    == Some(format!("{sequence:08}-{}.json", &digest[7..]).as_str()),
                "record filename/content/sequence mismatch",
            )?;
            require(
                authorship.get("sequence").and_then(Value::as_u64) == Some(sequence as u64)
                    && authorship.get("previous").and_then(Value::as_str) == previous.as_deref(),
                "append chain is incomplete or forked",
            )?;
            let native = native_errors(&record);
            require(
                native.is_empty(),
                &format!("invalid native invariants: {}", native.join("; ")),
            )?;
            if let Some((_, last)) = rows.last() {
                require(
                    instant(registered_at(&record))? >= instant(registered_at(last))?,
                    "registration clock regressed",
                )?;
            }
            previous = Some(digest);
            rows.push((path, record));
        }
        let requests: HashSet<&str> = rows
            .iter()
            .filter_map(|(_, record)| {
                record
                    .get("authorship")
                    .and_then(|authorship| authorship.get("request_id"))
                    .and_then(Value::as_str)
            })
            .collect();
        require(requests.len() == rows.len(), "duplicate idempotency key")?;
        Ok(rows)
    }

    pub fn records(&self) -> Result<Vec<Value>> {
        Ok(self
            .entries()?
            .into_iter()
            .map(|(_, record)| record)
            .collect())
    }

    /// Every unsuperseded revision of one identity, sorted. Conflicts stay explicit.
    pub fn heads(&self, ident: &str) -> Result<Vec<String>> {
        let records = self.records()?;
        heads_of(&records, ident)
    }

    /// An exact committed snapshot of one owned revision, verified against the append.
    pub fn pin(&self, ident: &str, revision: &str, source_revision: &str) -> Result<Value> {
        let matches: Vec<(PathBuf, Value)> = self
            .entries()?
            .into_iter()
            .filter(|(_, record)| {
                text(record, "id") == Some(ident) && text(record, "revision_id") == Some(revision)
            })
            .collect();
        require(
            matches.len() == 1,
            "exact identity/revision is missing or ambiguous",
        )?;
        let (path, record) = matches
            .into_iter()
            .next()
            .ok_or_else(|| OwnerError::Invalid("exact identity/revision is missing".into()))?;
        let relative = path
            .strip_prefix(self.checkout.root())
            .map_err(|_| OwnerError::Invalid("record escaped the owner root".into()))?
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/");
        self.snapshot(&self.checkout, &relative, source_revision, Some(&record))
    }

    /// Read one committed record, verifying the native receipt chain that leads to it.
    fn snapshot(
        &self,
        checkout: &Checkout,
        path: &str,
        revision: &str,
        expected: Option<&Value>,
    ) -> Result<Value> {
        checkout.safe_path(path)?;
        let data = checkout.git_bytes(revision, path)?;
        let mut record = parse_json(&data)?;
        require(
            validate_structural(&record).is_empty(),
            "invalid pinned record",
        )?;
        if record.get("schema_version").and_then(Value::as_u64) == Some(2) {
            NativeReceipt::new(&record, path).verify_chain(checkout, revision)?;
        }
        if let Some(expected) = expected {
            require(
                canonical_json(&record)? == canonical_json(expected)?,
                "committed record differs from canonical append",
            )?;
        }
        let blob_oid = checkout.text(&["rev-parse", &format!("{revision}:{path}")]);
        let provenance = record
            .get_mut("provenance")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| OwnerError::Invalid("pinned record lacks provenance".into()))?;
        provenance.insert("git_revision".into(), json!(revision));
        provenance.insert("blob_oid".into(), blob_oid.map_or(Value::Null, Value::from));
        provenance.insert("sha256".into(), json!(digest_bytes(&data)));
        provenance.insert("path".into(), json!(path));
        provenance.insert("selector".into(), json!("$"));
        provenance.insert("working_tree".into(), json!(false));
        Ok(record)
    }

    /// Read only the explicitly routed owner and exact Git snapshot; never a HEAD fallback.
    pub fn resolve(&self, reference: &Value) -> Result<Value> {
        let identity = text(reference, "id").unwrap_or_default();
        let repository = text(reference, "repository").unwrap_or_default();
        let source_revision = text(reference, "source_revision").unwrap_or_default();
        if let Some(resolver) = &self.artifact_resolver
            && let Some(proof) = resolver.resolve(reference)?
        {
            let record = proof.verify()?;
            require(
                provenance(&record, "repository") == Some(repository)
                    && record_key(&record) == reference_key(reference),
                "artifact resolver returned a different exact pin",
            )?;
            return Ok(record);
        }
        let checkout = self.sources.get(repository);
        require(
            checkout.is_some() && !source_revision.is_empty(),
            &format!("unrouted or unpinned reference: {identity}"),
        )?;
        let checkout = checkout.ok_or_else(|| {
            OwnerError::Invalid(format!("unrouted or unpinned reference: {identity}"))
        })?;
        // A rebuildable search over JSON in the specified snapshot, no secondary authority.
        let listing = checkout
            .listing(source_revision)
            .ok_or_else(|| OwnerError::Invalid("source revision unavailable".into()))?;
        let revision_id = text(reference, "revision_id").unwrap_or_default();
        let mut matches = Vec::new();
        for path in listing.lines() {
            if !path.ends_with(".json") || !path.starts_with("research/") {
                continue;
            }
            let Ok(bytes) = checkout.git_bytes(source_revision, path) else {
                continue;
            };
            let Ok(candidate) = parse_json(&bytes) else {
                continue;
            };
            if text(&candidate, "id") == Some(identity)
                && text(&candidate, "revision_id") == Some(revision_id)
            {
                matches.push(self.snapshot(checkout, path, source_revision, None)?);
            }
        }
        require(
            matches.len() == 1,
            &format!("exact reference missing or ambiguous: {identity}"),
        )?;
        let record = matches.into_iter().next().unwrap_or(Value::Null);
        require(
            provenance(&record, "repository") == Some(repository),
            "source namespace mismatch",
        )?;
        Ok(record)
    }

    /// Resolve the complete evidence closure; pending references stay explicit missingness.
    pub fn closure(&self, records: &[Value]) -> Result<(Vec<Value>, Vec<Value>)> {
        let mut known = Ordered::default();
        for record in records {
            known.insert(record_key(record), record.clone());
        }
        let mut pending = Vec::new();
        let mut queue: Vec<Value> = records.to_vec();
        while let Some(record) = queue.pop() {
            for reference in record_references(&record) {
                let key = reference_key(reference);
                if known.contains(&key) {
                    continue;
                }
                match self.resolve(reference) {
                    Ok(target) => {
                        known.insert(key, target.clone());
                        queue.push(target);
                    }
                    Err(error) => {
                        if text(reference, "status") == Some("resolved") {
                            return Err(error);
                        }
                        pending.push(json!({"reference": reference, "reason": error.to_string()}));
                    }
                }
            }
        }
        Ok((known.into_values(), pending))
    }

    /// Append one immutable record for an authoring operation.
    pub fn apply(&self, operation: &str, request: &Value) -> Result<Value> {
        let operation_kind = OPERATIONS
            .iter()
            .find(|(name, _)| *name == operation)
            .map(|(_, kind)| *kind);
        require(
            operation_kind.is_some() && request.is_object(),
            "unknown operation or invalid request",
        )?;
        let fields: BTreeSet<&str> = request
            .as_object()
            .map(|object| object.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let unknown: Vec<&str> = fields
            .iter()
            .copied()
            .filter(|field| !REQUEST_FIELDS.contains(field))
            .collect();
        require(
            unknown.is_empty(),
            &format!("unknown request fields: {}", unknown.join(", ")),
        )?;
        for field in [
            "request_id",
            "id",
            "scope",
            "payload",
            "orbit_links",
            "expected_heads",
            "reason",
        ] {
            require(fields.contains(field), &format!("request requires {field}"))?;
        }
        let request_id = text(request, "request_id").unwrap_or_default();
        require(!request_id.trim().is_empty(), "idempotency key required")?;
        let expected_heads = request
            .get("expected_heads")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                OwnerError::Invalid(
                    "expected_heads must explicitly list all current revisions".into(),
                )
            })?
            .clone();
        let request_digest = digest_bytes(&canonical_json(
            &json!({"operation": operation, "request": request}),
        )?);

        let _lock = self.locked()?;
        let entries = self.entries()?;
        for (_, record) in &entries {
            let authorship = record.get("authorship").cloned().unwrap_or(Value::Null);
            if text(&authorship, "request_id") == Some(request_id) {
                require(
                    text(&authorship, "request_digest") == Some(request_digest.as_str()),
                    "idempotency key reused for different request",
                )?;
                return Ok(record.clone());
            }
        }
        let kind = operation_kind
            .flatten()
            .or_else(|| text(request, "kind"))
            .unwrap_or_default()
            .to_owned();
        require(KINDS.contains(&kind.as_str()), "invalid kind")?;
        let legacy_id = legacy_identifier(request)?;
        let ident = format!(
            "urn:research:{}:{kind}:{}",
            self.repository,
            percent_encode(&legacy_id)
        );
        let known_records: Vec<Value> = entries.iter().map(|(_, record)| record.clone()).collect();
        let previous_records: Vec<&Value> = known_records
            .iter()
            .filter(|record| text(record, "id") == Some(ident.as_str()))
            .collect();
        let heads = heads_of(&known_records, &ident)?;
        let mut requested: Vec<String> = expected_heads
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        requested.sort();
        require(
            requested.len() == expected_heads.len() && requested == heads,
            &format!(
                "stale base: expected {:?}, actual {heads:?}",
                expected_heads
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
            ),
        )?;
        let bases: Vec<String> = match request.get("supersedes") {
            Some(value) => {
                let listed = value.as_array().ok_or_else(|| {
                    OwnerError::Invalid(
                        "supersedes must select current heads; explicitly retain other conflicts"
                            .into(),
                    )
                })?;
                listed
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            }
            None => heads.clone(),
        };
        let unique: BTreeSet<&String> = bases.iter().collect();
        require(
            unique.len() == bases.len() && bases.iter().all(|base| heads.contains(base)),
            "supersedes must select current heads; explicitly retain other conflicts",
        )?;
        let parents: Vec<Value> = previous_records
            .iter()
            .filter(|record| {
                text(record, "revision_id")
                    .is_some_and(|revision| bases.iter().any(|base| base == revision))
            })
            .map(|record| (*record).clone())
            .collect();
        let head = self.checkout.head()?;
        let mut parent_refs = Vec::new();
        for parent in &parents {
            let pinned = self.pin(
                text(parent, "id").unwrap_or_default(),
                text(parent, "revision_id").unwrap_or_default(),
                &head,
            );
            parent_refs.push(match pinned {
                Ok(pinned) => reference(&pinned, "resolved"),
                Err(_) => reference(parent, "pending"),
            });
        }
        let timestamp = self.clock.now();
        if let Some((_, last)) = entries.last() {
            require(
                instant(&timestamp)? >= instant(registered_at(last))?,
                "registration clock regressed",
            )?;
        }
        let mut payload = request.get("payload").cloned().unwrap_or(Value::Null);
        match operation {
            "preregister" => self.prepare_freeze(&mut payload, &timestamp)?,
            "begin-run" => {
                self.prepare_run(&payload)?;
                self.prepare_start(&mut payload, &previous_records, &timestamp)?;
            }
            "record-run" => {
                self.prepare_run(&payload)?;
                self.prepare_result(&mut payload, request, &ident, &timestamp)?;
            }
            "retire" => {
                require(
                    ["program", "claim"].contains(&kind.as_str())
                        && parents.len() == 1
                        && parents.first().and_then(|parent| parent.get("payload"))
                            == Some(&payload),
                    "retirement preserves one program/claim revision payload; assessments remain intact",
                )?;
            }
            _ => {}
        }
        let provenance = json!({
            "repository": self.repository,
            "git_revision": head,
            "blob_oid": Value::Null,
            "sha256": request_digest,
            "path": self.relative,
            "selector": "$request",
            "historical": false,
            "working_tree": true,
        });
        let mut record = make_record(
            &ident,
            &kind,
            &legacy_id,
            &payload,
            &provenance,
            &Classification {
                activity: if operation == "retire" {
                    "retired"
                } else {
                    "active"
                },
                scope: text(request, "scope").unwrap_or("unknown"),
                limitations: request
                    .get("limitations")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            },
        );
        let previous_digest = match entries.last() {
            Some((_, last)) => Value::from(digest_bytes(&canonical_json(last)?)),
            None => Value::Null,
        };
        let object = record
            .as_object_mut()
            .ok_or_else(|| OwnerError::Invalid("record must be an object".into()))?;
        object.insert("schema_version".into(), json!(2));
        object.insert(
            "orbit_links".into(),
            request.get("orbit_links").cloned().unwrap_or(Value::Null),
        );
        object.insert(
            "references".into(),
            request
                .get("references")
                .cloned()
                .unwrap_or_else(|| json!([])),
        );
        object.insert(
            "presentation".into(),
            request
                .get("presentation")
                .cloned()
                .unwrap_or_else(|| json!({})),
        );
        // Supersedes is an owner-local revision link; exact publication pins are optional.
        object.insert(
            "authorship".into(),
            json!({
                "registered_at": timestamp,
                "sequence": entries.len() + 1,
                "previous": previous_digest,
                "request_id": request_id,
                "request_digest": request_digest,
                "supersedes": parent_refs,
                "reason": request.get("reason").cloned().unwrap_or(Value::Null),
            }),
        );
        let revision = revision_digest(&record)?;
        record
            .as_object_mut()
            .ok_or_else(|| OwnerError::Invalid("record must be an object".into()))?
            .insert("revision_id".into(), json!(revision));
        require(
            !previous_records
                .iter()
                .any(|previous| text(previous, "revision_id") == Some(revision.as_str())),
            "revision already frozen; presentation changes use linked prose or a new normative revision",
        )?;
        let mut errors = validate_structural(&record);
        errors.extend(native_errors(&record));
        require(errors.is_empty(), &errors.join("; "))?;
        let (targets, _) = self.closure(std::slice::from_ref(&record))?;
        let context: Vec<Value> = targets
            .into_iter()
            .filter(|target| record_key(target) != record_key(&record))
            .collect();
        let errors = validate_native(&record, &context);
        require(errors.is_empty(), &errors.join("; "))?;
        self.publish(&record)?;
        Ok(record)
    }

    /// `preregister` accepts semantic content only; freeze time is measured here.
    fn prepare_freeze(&self, payload: &mut Value, timestamp: &str) -> Result<()> {
        require(
            payload
                .as_object()
                .is_some_and(|object| object.len() == 1 && object.contains_key("semantic")),
            "preregister accepts semantic content only; no caller freeze dates or history",
        )?;
        let semantic = payload.get("semantic").cloned().unwrap_or(Value::Null);
        let object = payload
            .as_object_mut()
            .ok_or_else(|| OwnerError::Invalid("invalid protocol payload".into()))?;
        object.insert("semantic_digest".into(), json!(protocol_digest(&semantic)?));
        object.insert("freeze".into(), json!("registered"));
        object.insert("frozen_at".into(), json!(timestamp));
        object.insert("freeze_evidence".into(), Value::Null);
        let code = semantic.get("code").cloned().unwrap_or(Value::Null);
        let revision = text(&code, "git_revision").unwrap_or_default();
        let source = self
            .sources
            .get(text(&code, "repository").unwrap_or_default());
        require(
            source.is_some_and(|source| {
                source
                    .text(&["rev-parse", "--verify", &format!("{revision}^{{commit}}")])
                    .as_deref()
                    == Some(revision)
            }),
            "code revision must exist in an explicitly routed source checkout",
        )?;
        let holdout_digest = semantic
            .get("holdout")
            .and_then(|holdout| holdout.get("digest"))
            .cloned()
            .unwrap_or(Value::Null);
        let mut frozen_holdout = false;
        for reference in semantic
            .get("inputs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let input = self.resolve(&reference)?;
            frozen_holdout = frozen_holdout
                || (text(&input, "kind") == Some("artifact")
                    && input
                        .get("payload")
                        .and_then(|payload| payload.get("snapshot_digest"))
                        == Some(&holdout_digest));
        }
        require(
            frozen_holdout,
            "holdout or seed-plan digest must be an exact frozen input artifact",
        )
    }

    /// Run timestamps are observed, never supplied.
    fn prepare_run(&self, payload: &Value) -> Result<()> {
        require(
            payload.as_object().is_some_and(|object| {
                !object.contains_key("started_at") && !object.contains_key("finished_at")
            }),
            "run timestamps are observed, never supplied",
        )
    }

    /// A run-start receipt binds the frozen protocol after its evaluation boundary.
    fn prepare_start(
        &self,
        payload: &mut Value,
        previous_records: &[&Value],
        timestamp: &str,
    ) -> Result<()> {
        require(
            previous_records.is_empty()
                && text(payload, "execution_status") == Some("running")
                && payload.get("start").is_some_and(Value::is_null)
                && payload
                    .get("result_artifacts")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty),
            "begin-run needs a new running identity with no results",
        )?;
        let reference = payload.get("protocol").cloned().unwrap_or(Value::Null);
        let protocol = self.resolve(&reference)?;
        require(
            text(&protocol, "kind") == Some("protocol")
                && protocol.get("schema_version").and_then(Value::as_u64) == Some(2),
            "begin-run requires native frozen protocol",
        )?;
        let source_revision = text(&reference, "source_revision").unwrap_or_default();
        let protocol_root = self
            .sources
            .get(text(&reference, "repository").unwrap_or_default())
            .ok_or_else(|| OwnerError::Invalid("unrouted protocol reference".into()))?;
        require(
            protocol_root
                .text(&["merge-base", "--is-ancestor", source_revision, "HEAD"])
                .is_some(),
            "freeze commit must precede run registration",
        )?;
        let semantic = protocol
            .get("payload")
            .and_then(|payload| payload.get("semantic"))
            .cloned()
            .unwrap_or(Value::Null);
        let holdout = semantic.get("holdout").cloned().unwrap_or(Value::Null);
        require(
            payload.get("code") == semantic.get("code")
                && payload.get("inputs") == semantic.get("inputs")
                && payload.get("holdout_digest") == holdout.get("digest"),
            "start must bind exact frozen code/data/holdout",
        )?;
        require(
            instant(timestamp)?
                >= instant(text(&holdout, "evaluation_not_before").unwrap_or_default())?,
            "evaluation boundary has not arrived",
        )?;
        let object = payload
            .as_object_mut()
            .ok_or_else(|| OwnerError::Invalid("invalid run payload".into()))?;
        object.insert("started_at".into(), json!(timestamp));
        object.insert("finished_at".into(), Value::Null);
        Ok(())
    }

    /// A result receipt retains its start chronology, links and frozen pins.
    fn prepare_result(
        &self,
        payload: &mut Value,
        request: &Value,
        ident: &str,
        timestamp: &str,
    ) -> Result<()> {
        require(
            ["completed", "failed", "cancelled"]
                .contains(&text(payload, "execution_status").unwrap_or_default()),
            "record-run requires terminal execution status",
        )?;
        let reference = payload.get("start").cloned().unwrap_or(Value::Null);
        let start = if reference.is_null() {
            None
        } else {
            Some(self.resolve(&reference)?)
        };
        if let Some(start) = &start {
            let start_payload = start.get("payload").cloned().unwrap_or(Value::Null);
            require(
                text(start, "id") == Some(ident)
                    && text(start, "kind") == Some("experiment")
                    && text(&start_payload, "execution_status") == Some("running"),
                "start receipt must be for this run",
            )?;
            require(
                request.get("orbit_links") == start.get("orbit_links"),
                "run must retain exact starting Orbit provenance",
            )?;
            let deviations = payload
                .get("deviations")
                .and_then(Value::as_array)
                .is_some_and(|values| !values.is_empty());
            for field in ["protocol", "code", "inputs", "holdout_digest"] {
                require(
                    payload.get(field) == start_payload.get(field) || deviations,
                    &format!("run {field} differs from start; explicit deviations required"),
                )?;
            }
        }
        let started_at = start
            .as_ref()
            .and_then(|start| start.get("payload"))
            .and_then(|payload| payload.get("started_at"))
            .cloned()
            .unwrap_or(Value::Null);
        let object = payload
            .as_object_mut()
            .ok_or_else(|| OwnerError::Invalid("invalid run payload".into()))?;
        object.insert("started_at".into(), started_at);
        object.insert("finished_at".into(), json!(timestamp));
        Ok(())
    }

    /// Serialize once, fsync, then publish with an exclusive hard link. Nothing is replaced.
    fn publish(&self, record: &Value) -> Result<()> {
        let canonical = canonical_json(record)?;
        let digest = digest_bytes(&canonical);
        let sequence = record
            .get("authorship")
            .and_then(|authorship| authorship.get("sequence"))
            .and_then(Value::as_u64)
            .ok_or_else(|| OwnerError::Invalid("record lacks an append sequence".into()))?;
        let destination = self
            .directory
            .join(format!("{sequence:08}-{}.json", &digest[7..]));
        let staging = self.directory.join(staging_name());
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staging)?;
            file.write_all(&canonical)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);
            // Hard-link publication is atomic and never replaces an existing path.
            std::fs::hard_link(&staging, &destination)?;
            File::open(&self.directory)?.sync_all()?;
            Ok(())
        })();
        let removal = std::fs::remove_file(&staging);
        result.and(removal.map_err(OwnerError::from))
    }

    /// Exact dependencies, every assessment of this revision, and explicit unresolved links.
    pub fn trace(&self, ident: &str, revision: &str) -> Result<Value> {
        let records = self.records()?;
        let matches: Vec<Value> = records
            .iter()
            .filter(|record| {
                text(record, "id") == Some(ident) && text(record, "revision_id") == Some(revision)
            })
            .cloned()
            .collect();
        require(
            matches.len() == 1,
            "trace requires an exact identity/revision",
        )?;
        let (closure, mut pending) = self.closure(&matches)?;
        // Include every assessment of this revision, preserving disagreements and retirement.
        let assessments: Vec<Value> = records
            .iter()
            .filter(|record| {
                text(record, "kind") == Some("assessment")
                    && record
                        .get("payload")
                        .and_then(|payload| payload.get("claim"))
                        .is_some_and(|claim| {
                            text(claim, "id") == Some(ident)
                                && text(claim, "revision_id") == Some(revision)
                        })
            })
            .cloned()
            .collect();
        let (assessed, more) = self.closure(&assessments)?;
        pending.extend(more);
        let mut merged = Ordered::default();
        for record in closure.into_iter().chain(assessed) {
            merged.insert(record_key(&record), record);
        }
        Ok(json!({
            "schema_version": 2,
            "kind": "trace",
            "root": {"id": ident, "revision_id": revision},
            "heads": heads_of(&records, ident)?,
            "records": merged.into_values(),
            "unresolved": pending,
        }))
    }

    /// Bind every canonical append to one commit and export its validated closure.
    pub fn export(&self, revision: &str) -> Result<Value> {
        let mut records = Vec::new();
        for record in self.records()? {
            records.push(self.pin(
                text(&record, "id").unwrap_or_default(),
                text(&record, "revision_id").unwrap_or_default(),
                revision,
            )?);
        }
        require(!records.is_empty(), "no native records to export")?;
        let (closure, pending) = self.closure(&records)?;
        for record in &closure {
            let context: Vec<Value> = closure
                .iter()
                .filter(|other| record_key(other) != record_key(record))
                .cloned()
                .collect();
            let errors = validate_native(record, &context);
            require(
                errors.is_empty(),
                &format!("export validation: {}", errors.join("; ")),
            )?;
        }
        // v1 manifests intentionally allow one source revision per repository. Group exact
        // historical snapshots instead of replacing all links with latest HEAD.
        let mut pins: Vec<(String, Option<String>)> = closure
            .iter()
            .map(|record| {
                (
                    provenance(record, "repository")
                        .unwrap_or_default()
                        .to_owned(),
                    provenance(record, "git_revision").map(str::to_owned),
                )
            })
            .collect();
        pins.sort();
        pins.dedup();
        let mut manifests = Vec::new();
        for (repository, pin) in pins {
            let subset: Vec<&Value> = closure
                .iter()
                .filter(|record| {
                    provenance(record, "repository") == Some(repository.as_str())
                        && provenance(record, "git_revision") == pin.as_deref()
                })
                .collect();
            let manifest = json!({
                "schema_version": 1,
                "kind": "manifest",
                "repositories": [{"id": repository, "git_revision": pin}],
                "references": subset
                    .iter()
                    .map(|record| reference(record, "pending"))
                    .collect::<Vec<_>>(),
            });
            let manifest = reconcile(&manifest, &closure)?;
            require(
                validate(&manifest, &closure).is_empty(),
                "invalid export manifest",
            )?;
            manifests.push(manifest);
        }
        Ok(json!({
            "schema_version": 2,
            "kind": "export",
            "records": closure,
            "manifests": manifests,
            "unresolved": pending,
        }))
    }
}

/// One native receipt and the chain of predecessors that must precede it in a commit.
pub struct NativeReceipt<'a> {
    record: &'a Value,
    path: &'a str,
}

impl<'a> NativeReceipt<'a> {
    pub fn new(record: &'a Value, path: &'a str) -> Self {
        Self { record, path }
    }

    /// Walk the committed append chain back to sequence one, refusing gaps and forks.
    pub fn verify_chain(&self, checkout: &Checkout, revision: &str) -> Result<()> {
        let mut current = self.record.clone();
        let mut current_path = PathBuf::from(self.path);
        loop {
            let authorship = current.get("authorship").cloned().unwrap_or(Value::Null);
            let sequence = authorship
                .get("sequence")
                .and_then(Value::as_u64)
                .ok_or_else(|| OwnerError::Invalid("native receipt lacks a sequence".into()))?;
            let digest = digest_bytes(&canonical_json(&current)?);
            require(
                current_path.file_name().and_then(|name| name.to_str())
                    == Some(format!("{sequence:08}-{}.json", &digest[7..]).as_str()),
                "pinned native receipt filename/content mismatch",
            )?;
            let previous = authorship.get("previous").and_then(Value::as_str);
            if sequence == 1 {
                require(
                    previous.is_none(),
                    "first native receipt cannot have a predecessor",
                )?;
                return Ok(());
            }
            let previous = previous.ok_or_else(|| {
                OwnerError::Invalid("native receipt chain is missing a predecessor".into())
            })?;
            current_path = current_path.parent().unwrap_or(Path::new("")).join(format!(
                "{:08}-{}.json",
                sequence - 1,
                &previous[7..]
            ));
            let relative = current_path
                .components()
                .filter_map(|component| component.as_os_str().to_str())
                .collect::<Vec<_>>()
                .join("/");
            let parent = parse_json(&checkout.git_bytes(revision, &relative)?)?;
            require(
                validate_structural(&parent).is_empty()
                    && parent.get("schema_version").and_then(Value::as_u64) == Some(2),
                "invalid pinned native predecessor",
            )?;
            require(
                parent
                    .get("authorship")
                    .and_then(|authorship| authorship.get("sequence"))
                    .and_then(Value::as_u64)
                    == Some(sequence - 1)
                    && provenance(&parent, "repository") == provenance(&current, "repository"),
                "pinned native predecessor sequence/owner mismatch",
            )?;
            require(
                instant(registered_at(&parent))? <= instant(registered_at(&current))?,
                "pinned registration clock regressed",
            )?;
            current = parent;
        }
    }
}

/// Publish one complete document atomically; no truncation of an existing destination.
pub fn write_json_new(document: &Value, path: &Path) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let staging = parent.join(format!(".research-export-{}", staging_suffix()));
    let canonical = canonical_json(document)?;
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)?;
        file.write_all(&canonical)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        std::fs::hard_link(&staging, path)?;
        Ok(())
    })();
    let removal = std::fs::remove_file(&staging);
    result.and(removal.map_err(OwnerError::from))
}

/// An exact scientific reference to one record snapshot.
pub fn reference(record: &Value, status: &str) -> Value {
    json!({
        "repository": provenance(record, "repository"),
        "id": text(record, "id"),
        "revision_id": text(record, "revision_id"),
        "source_revision": record
            .get("provenance")
            .and_then(|provenance| provenance.get("git_revision"))
            .cloned()
            .unwrap_or(Value::Null),
        "status": status,
    })
}

/// Structural, native and confirmatory validation of one record against its closure.
pub fn validate_native(record: &Value, targets: &[Value]) -> Vec<String> {
    let mut errors = validate(record, targets);
    if record.get("schema_version").and_then(Value::as_u64) == Some(2) {
        errors.extend(native_errors(record));
    }
    let mut known: HashMap<RecordKey, Value> = HashMap::new();
    for value in std::iter::once(record).chain(targets) {
        known.insert(record_key(value), value.clone());
    }
    // Validate the entire supplied evidence closure, without recursively rebuilding it.
    for value in std::iter::once(record).chain(targets) {
        if value.get("schema_version").and_then(Value::as_u64) == Some(2) {
            errors.extend(native_reference_errors(value, &known));
        }
        if text(value, "kind") == Some("assessment")
            && value
                .get("payload")
                .and_then(|payload| text(payload, "inference"))
                == Some("confirmatory-primary")
        {
            errors.extend(confirmation_errors(value, &known, &HashSet::new()));
        }
    }
    errors
}

/// Owner-declared classification for one append; `unknown` values become explicit missingness.
struct Classification<'a> {
    activity: &'a str,
    scope: &'a str,
    limitations: Value,
}

fn make_record(
    ident: &str,
    kind: &str,
    legacy_id: &str,
    payload: &Value,
    provenance: &Value,
    classification: &Classification<'_>,
) -> Value {
    let Classification {
        activity,
        scope,
        limitations,
    } = classification;
    let mut missingness = Vec::new();
    for (field, value) in [
        ("activity", Some(*activity)),
        ("scope", Some(*scope)),
        (
            "git-revision",
            provenance.get("git_revision").and_then(Value::as_str),
        ),
    ] {
        if value.is_none_or(|value| value == "unknown") {
            missingness.push(Value::from(field));
        }
    }
    json!({
        "schema_version": 1,
        "kind": kind,
        "id": ident,
        "aliases": [legacy_id],
        "activity": activity,
        "scope": scope,
        "provenance": provenance,
        "limitations": limitations,
        "missingness": missingness,
        "legacy": Value::Null,
        "references": [],
        "presentation": {},
        "payload": payload,
        "revision_id": Value::Null,
    })
}

fn heads_of(records: &[Value], ident: &str) -> Result<Vec<String>> {
    let owned: Vec<&Value> = records
        .iter()
        .filter(|record| text(record, "id") == Some(ident))
        .collect();
    let superseded: HashSet<&str> = owned
        .iter()
        .flat_map(|record| {
            record
                .get("authorship")
                .and_then(|authorship| authorship.get("supersedes"))
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
        })
        .filter(|reference| text(reference, "id") == Some(ident))
        .filter_map(|reference| text(reference, "revision_id"))
        .collect();
    let mut heads: Vec<String> = owned
        .iter()
        .filter_map(|record| text(record, "revision_id"))
        .filter(|revision| !superseded.contains(revision))
        .map(str::to_owned)
        .collect();
    heads.sort();
    Ok(heads)
}

fn legacy_identifier(request: &Value) -> Result<String> {
    match request.get("id") {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(Value::Number(value)) => Ok(value.to_string()),
        _ => Err(OwnerError::Invalid(
            "request id must be an explicit owner identifier".into(),
        )),
    }
}

/// Percent-encode exactly like CPython `urllib.parse.quote(value, safe="")`.
fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn is_append_name(name: &str) -> bool {
    let Some((sequence, tail)) = name.split_once('-') else {
        return false;
    };
    let Some(digest) = tail.strip_suffix(".json") else {
        return false;
    };
    sequence.len() == 8
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
        && digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn staging_name() -> String {
    format!(".append-{}", staging_suffix())
}

fn staging_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

fn registered_at(record: &Value) -> &str {
    record
        .get("authorship")
        .and_then(|authorship| authorship.get("registered_at"))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn provenance<'a>(record: &'a Value, key: &str) -> Option<&'a str> {
    record
        .get("provenance")
        .and_then(|provenance| provenance.get(key))
        .and_then(Value::as_str)
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Insertion-ordered record set: closures and exports stay reproducible.
#[derive(Default)]
struct Ordered {
    index: HashMap<RecordKey, usize>,
    values: Vec<Value>,
}

impl Ordered {
    fn contains(&self, key: &RecordKey) -> bool {
        self.index.contains_key(key)
    }

    fn insert(&mut self, key: RecordKey, value: Value) {
        match self.index.get(&key) {
            Some(position) => self.values[*position] = value,
            None => {
                self.index.insert(key, self.values.len());
                self.values.push(value);
            }
        }
    }

    fn into_values(self) -> Vec<Value> {
        self.values
    }
}

/// An exclusive advisory lock on the records directory inode.
struct DirectoryLock {
    descriptor: i32,
}

impl DirectoryLock {
    fn acquire(directory: &Path) -> Result<Self> {
        let path = CString::new(directory.as_os_str().as_bytes())
            .map_err(|_| OwnerError::Invalid("records directory path is not usable".into()))?;
        // SAFETY: `path` is a valid NUL-terminated C string for the duration of the call.
        let descriptor = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: `descriptor` is an open file descriptor owned by this value.
        if unsafe { libc::flock(descriptor, libc::LOCK_EX) } != 0 {
            let error = std::io::Error::last_os_error();
            // SAFETY: closing the descriptor we just opened.
            unsafe { libc::close(descriptor) };
            return Err(error.into());
        }
        Ok(Self { descriptor })
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        // SAFETY: `descriptor` is owned here and closed exactly once; closing releases flock.
        unsafe { libc::close(self.descriptor) };
    }
}
