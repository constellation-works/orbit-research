"""Read-only adapters. Unknown meaning is an exception, not a guessed fact."""
from __future__ import annotations

from collections import Counter
from contextlib import closing
from hashlib import sha256
import json
import math
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import tempfile

from .contract import digest_bytes, make_record, protocol_digest, reference, revision_digest, validate

PATTERNS = {
    'principia': ['theory/**/claims.json', 'theory/**/*.md', 'gates/*.json',
                  'studies/*preregistration*.md', 'ledger.md'],
    'parallax': ['docs/*.md', 'data/**/*.db', 'data/**/*.sqlite', 'data/**/*.sqlite3',
                 'artifacts/**/*.db', 'artifacts/**/*.sqlite', 'artifacts/**/*.sqlite3'],
    'orrery': ['lab/sims/**/*.json'],
    'astrolabe': ['data/processed/**/*.json'],
}
VERDICTS = {'supported':'supported', 'refuted':'refuted', 'mixed':'inconclusive',
            'inconclusive':'inconclusive', 'conditional':'conditional',
            'untested':'untested', 'conjecture':'untested'}
ACTIVITY = {'active', 'paused', 'retired', 'resolved'}
MAX_METADATA_BYTES = 8 * 1024 * 1024


def file_digest(path):
    h = sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return 'sha256:' + h.hexdigest()


def strict_json(data):
    def pairs(values):
        result = {}
        for key, value in values:
            if key in result:
                raise ValueError(f'duplicate JSON key: {key}')
            result[key] = value
        return result
    def nonfinite(value):
        raise ValueError(f'non-finite JSON number: {value}')
    return json.loads(data, object_pairs_hook=pairs, parse_constant=nonfinite)


def git(root, *args):
    p = subprocess.run(['git', '-C', str(root), *args], capture_output=True, text=True,
                       env={**os.environ, 'GIT_OPTIONAL_LOCKS':'0'})
    return p.stdout.strip() if p.returncode == 0 else None


def contained(root, path):
    return path.resolve().is_relative_to(root)


def discover(root, adapter, selected):
    if selected:
        paths = []
        for rel in selected:
            path = root / rel
            if Path(rel).is_absolute() or not contained(root, path) or any(p in {'.git', '.orbit'} for p in Path(rel).parts):
                raise ValueError(f'selected input must remain in scientific source root: {rel}')
            if not path.is_file():
                raise ValueError(f'selected input is not a file: {rel}')
            paths.append(path)
        return sorted(set(paths)), list(selected)
    return sorted({p for pattern in PATTERNS[adapter] for p in root.glob(pattern)
                   if p.is_file() and not any(x in {'.git', '.orbit'} for x in p.relative_to(root).parts)}), PATTERNS[adapter]


def _nested_records(value, selector='$'):
    """Inventory structured list members; scalar arrays stay in their complete container."""
    if isinstance(value, dict):
        for key, child in value.items():
            yield from _nested_records(child, selector + '/' + key.replace('~', '~0').replace('/', '~1'))
    elif isinstance(value, list):
        for n, child in enumerate(value):
            loc = f'{selector}/{n}'
            if isinstance(child, dict):
                yield loc, child
            yield from _nested_records(child, loc)


class Importer:
    def __init__(self, root, adapter, repository, revision):
        self.root, self.adapter, self.repository, self.revision = root, adapter, repository, revision
        self.inventory, self.records, self.files = [], [], []
        self.before = {}

    def track(self, path):
        if not contained(self.root, path):
            raise ValueError(f'source symlink escapes root: {path.relative_to(self.root)}')
        if path not in self.before:
            self.before[path] = file_digest(path) if path.exists() else None
        return self.before[path]

    def provenance(self, path, selector='$'):
        digest = self.track(path)
        rel = path.relative_to(self.root).as_posix()
        blob = git(self.root, 'rev-parse', f'{self.revision}:{rel}') if self.revision else None
        # rev-parse can echo an invalid object expression on failure; git() rejects it.
        actual = git(self.root, 'hash-object', '--', rel)
        return dict(repository=self.repository, git_revision=self.revision, blob_oid=blob,
                    sha256=digest, path=rel, selector=selector, historical=True,
                    working_tree=blob is None or blob != actual)

    def item(self, path, selector, raw, records=(), issues=()):
        accepted = []
        issues = list(issues)
        for record in records:
            errors = validate(record)
            if errors:
                issues.append(('invalid-candidate', '; '.join(errors)))
            else:
                accepted.append(record)
        records = accepted
        rel = path.relative_to(self.root).as_posix()
        self.inventory.append(dict(key=rel+'#'+selector, path=rel, selector=selector,
                                   sha256=self.before.get(path), raw=raw,
                                   disposition='exception' if issues else 'mapped',
                                   candidate_ids=[r['id'] for r in records],
                                   exceptions=[dict(code=c, message=m) for c, m in issues]))
        self.records.extend(records)

    def record(self, path, kind, ident, payload, raw, selector='$', scope='unknown', activity='unknown', missing=()):
        return make_record(self.repository, kind, ident, payload, self.provenance(path, selector),
                           legacy=raw, scope=scope, activity=activity, missingness=missing,
                           limitations=['Historical candidate; owner reconciliation required.'])

    def source_artifact(self, path, raw, selector='$', role='source'):
        # Content identity is appropriate for anonymous immutable source artifacts, never path identity.
        ident = digest_bytes((self.before[path] + selector).encode()).split(':')[1]
        return self.record(path, 'artifact', ident,
                           dict(role=role, availability='available', snapshot_digest=self.before[path],
                                locator=path.relative_to(self.root).as_posix(), media_type='application/json' if path.suffix == '.json' else 'text/plain'),
                           raw, selector)

    def json_file(self, path, raw):
        if not isinstance(raw, dict):
            self.item(path, '$', raw, issues=[('ambiguous-shape', 'Expected object; entire value retained.')])
            for selector, child in _nested_records(raw):
                self.item(path, selector, child, issues=[('unmapped-member', 'Member of unrecognized JSON container retained.')])
            return
        if self.adapter == 'principia' and path.name == 'claims.json':
            self.claims(path, raw)
        elif self.adapter == 'principia' and path.parent.name == 'gates' and isinstance(raw.get('id'), str):
            semantic = dict(raw)  # Historical gate is not a verified normative protocol extraction.
            rec = self.record(path, 'protocol', raw['id'], dict(semantic=semantic,
                              semantic_digest=protocol_digest(semantic), freeze='historical-unverified',
                              frozen_at=None, freeze_evidence=None), raw)
            self.item(path, '$', raw, [rec], [('historical-protocol', 'Gate text preserved; normative freeze and chronology require owner verification.')])
        elif self.adapter == 'orrery' and path.name == 'sim.json' and isinstance(raw.get('slug'), str):
            rec = self.record(path, 'program', raw['slug'], dict(role='program', title=raw.get('title') or raw['slug']), raw,
                              scope='simulation-under-assumptions', activity=raw.get('status') if raw.get('status') in ACTIVITY else 'unknown')
            self.item(path, '$', raw, [rec], [('catalog-not-run', 'Simulation catalog is an activity; execution, protocol and evidence references require reconciliation.')])
        elif self.adapter == 'astrolabe' and isinstance(raw.get('name'), str):
            self.dataset(path, raw)
        else:
            rec = self.source_artifact(path, raw, role='result' if self.adapter == 'orrery' else 'source')
            self.item(path, '$', raw, [rec], [('opaque-json', 'Source artifact preserved; no generic result-to-assessment or execution inference.')])
        # Claims have separately mapped members; other nested arrays are explicitly inventoried.
        for selector, child in _nested_records(raw):
            if self.adapter == 'principia' and path.name == 'claims.json' and re.fullmatch(r'\$/claims/\d+', selector):
                continue
            self.item(path, selector, child, issues=[('retained-member', 'Nested member retained verbatim in source; no independent scientific identity inferred.')])

    def claims(self, path, raw):
        records = []
        if isinstance(raw.get('doc'), str) and raw['doc'] and isinstance(raw.get('title'), str):
            records.append(self.record(path, 'program', raw['doc'], dict(role='theory', title=raw['title']), raw,
                                       activity=raw.get('status') if raw.get('status') in ACTIVITY else 'unknown'))
        self.item(path, '$', raw, records, [('legacy-program-state', 'Theory status retained; exploratory/growing/refuted do not imply activity or scientific support.')])
        if not isinstance(raw.get('claims'), list):
            self.item(path, '$/claims', raw.get('claims'), issues=[('missing-claims', 'claims must be an array; missingness retained.')])
            return
        for n, claim in enumerate(raw['claims']):
            loc = f'$/claims/{n}'
            if not isinstance(claim, dict) or not isinstance(claim.get('id'), str) or not claim['id'] or not isinstance(claim.get('claim'), str) or not claim['claim']:
                self.item(path, loc, claim, issues=[('malformed-claim', 'Claim needs nonempty legacy id and exact statement.')])
                continue
            domain = {'nature':'nature', 'derived':'model', 'model-property':'model', 'postulate':'model'}.get(claim.get('kind'), 'unknown')
            scope = 'derivation' if claim.get('kind') == 'derived' else 'unknown'
            rec = self.record(path, 'claim', claim['id'], dict(role='postulate' if claim.get('kind') == 'postulate' else 'claim', statement=claim['claim'], domain=domain), claim, loc, scope,
                              missing=['scope'] if scope == 'unknown' else [])
            assessment = self.record(path, 'assessment', claim['id'] + ':legacy-verdict',
                                     dict(claim=reference(rec), verdict=VERDICTS.get(claim.get('status'), 'unknown'),
                                          inference='historical', controls='unknown', basis='legacy-report',
                                          rationale=claim.get('evidence') or 'No source evidence rationale supplied.', evidence=[],
                                          legacy_verdict=claim.get('status') if isinstance(claim.get('status'), str) else None), claim, loc, scope,
                                     missing=['verified-evidence', 'controls'])
            issues = [('pending-evidence', 'Exact claim/verdict retained; evidence, scope and controls are not independently verified.')]
            self.item(path, loc, claim, [rec, assessment], issues)

    def dataset(self, path, raw):
        artifact = path.with_suffix('.parquet')
        snapshot = self.track(artifact)
        kind = raw.get('kind')
        if not isinstance(kind, str) or not kind:
            self.item(path, '$', raw, issues=[('ambiguous-dataset', 'Dataset kind absent; path is not a substitute identity.')])
            return
        rec = self.record(path, 'artifact', kind + ':' + raw['name'],
                          dict(role='dataset', availability='available' if snapshot else 'missing', snapshot_digest=snapshot,
                               locator=artifact.relative_to(self.root).as_posix(), media_type='application/vnd.apache.parquet'), raw,
                          missing=[] if snapshot else ['dataset-snapshot'])
        issues = []
        if not snapshot:
            issues.append(('missing-artifact', 'Sidecar survives but dataset bytes are unavailable.'))
        if raw.get('lineage'):
            issues.append(('pending-lineage', 'Legacy dataset names/timestamps are not exact revision pins; null parents remain missing.'))
        if 'lineage' not in raw:
            rec['missingness'].append('lineage-not-recorded')
        rec['revision_id'] = revision_digest(rec)
        self.item(path, '$', raw, [rec], issues)

    def markdown(self, path, content):
        source = self.source_artifact(path, content)
        self.item(path, '$', content, [source], [('prose-preserved', 'Whole prose, frontmatter, ledgers and limitations retained; no inferred freeze or verdict.')])
        if self.adapter != 'parallax':
            return
        # Conservative grammar: exact standalone R/H/E IDs in pipe-table first cells.
        for n, line in enumerate(content.splitlines(keepends=True), 1):
            match = re.match(r'^\|\s*([RHE][0-9]+)\s*\|', line)
            if not match:
                continue
            ident = match[1]
            cells = re.split(r'(?<!\\)\|', line)
            loc = f'line:{n}'
            if len(cells) < 4 or not cells[2].strip():
                self.item(path, loc, line, issues=[('ambiguous-table', 'Malformed R/H/E definition row retained.')])
            elif ident.startswith('H'):
                rec = self.record(path, 'claim', ident, dict(role='hypothesis', statement=cells[2].strip(), domain='empirical'), line, loc,
                                  scope='observation', missing=['verdict', 'activity', 'protocol'])
                self.item(path, loc, line, [rec])
            else:
                self.item(path, loc, line, issues=[('unmapped-register-row', 'R/E row retained; design prose does not establish a run or its outcome.')])
        # Headings that declare one exact ID are inventoried, never confused with H1–H2 discussion.
        for n, line in enumerate(content.splitlines(keepends=True), 1):
            if re.match(r'^#{1,6}\s+[RHE][0-9]+\s*[:.]\s', line):
                self.item(path, f'heading:{n}', line, issues=[('unmapped-register-heading', 'Definition/discussion heading retained with its full source document.')])

    def database(self, path):
        # Copy stable bytes BEFORE sqlite opens anything. WAL recovery happens only in a
        # private temporary directory. immutable=1 on the source would silently omit WAL.
        sidecars = [Path(str(path) + suffix) for suffix in ('-wal', '-shm', '-journal')]
        for p in sidecars:
            self.track(p)
        if sidecars[2].exists():
            self.item(path, '$', None, issues=[('sqlite-journal', 'Rollback journal present; request a quiescent owner snapshot.')])
            return
        with tempfile.TemporaryDirectory(prefix='orbit-research-sqlite-') as tmp:
            dest = Path(tmp) / 'snapshot.db'
            for source, target in [(path, dest), (sidecars[0], Path(str(dest)+'-wal'))]:
                if source.exists():
                    with source.open('rb') as src, target.open('wb') as dst:
                        for block in iter(lambda: src.read(1024 * 1024), b''):
                            dst.write(block)
                    if file_digest(target) != self.before[source]:
                        raise ValueError('source changed during SQLite snapshot')
            for p in [path, *sidecars]:
                if (file_digest(p) if p.exists() else None) != self.before[p]:
                    raise ValueError('source changed during SQLite snapshot')
            with closing(sqlite3.connect(dest.as_uri()+'?mode=ro', uri=True)) as conn:
                conn.execute('PRAGMA query_only=ON')
                conn.row_factory = sqlite3.Row
                tables = conn.execute("SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").fetchall()
                self.item(path, '$', [dict(t) for t in tables], issues=[('sqlite-schema', 'All user table definitions retained; every row separately inventoried.')])
                for table in tables:
                    name = table['name']
                    rows = conn.execute('SELECT * FROM "' + name.replace('"', '""') + '"')
                    for n, row in enumerate(rows):
                        raw = {k: {'sqlite_blob_hex':v.hex()} if isinstance(v, bytes) else v for k,v in dict(row).items()}
                        loc = f'table:{name}/row:{n}'
                        nonfinite = [k for k,v in raw.items() if isinstance(v, float) and not math.isfinite(v)]
                        if nonfinite:
                            for k in nonfinite:
                                raw[k] = {'sqlite_nonfinite': repr(raw[k])}
                            self.item(path, loc, raw, issues=[('nonfinite-sqlite-row', 'Nonfinite SQL value retained with explicit encoding; no candidate emitted.')])
                            continue
                        if name in {'trade_intents', 'research_intents'} and isinstance(raw.get('id'), str) and isinstance(raw.get('hypothesis'), str) and raw['hypothesis']:
                            rec = self.record(path, 'claim', 'journal:'+raw['id'], dict(role='hypothesis', statement=raw['hypothesis'], domain='empirical'), raw, loc,
                                              scope='observation', missing=['verified-freeze', 'verdict'])
                            self.item(path, loc, raw, [rec], [('historical-intent', 'Intent timestamp/rules retained; no prospective preregistration fabricated.')])
                        elif name in {'trade_outcomes', 'research_outcomes'} and isinstance(raw.get('trade_id'), str):
                            rec = self.record(path, 'experiment', 'journal:'+raw['trade_id'],
                                              dict(execution_status='completed', controls='unknown', protocol=None, result_artifacts=[]), raw, loc,
                                              scope='observation', missing=['protocol', 'controls', 'assessment'])
                            self.item(path, loc, raw, [rec], [('outcome-not-support', 'Recorded outcome is execution history; free-text result is not a scientific verdict.')])
                        else:
                            self.item(path, loc, raw, issues=[('unknown-journal-row', 'Unknown table/row shape retained without invented mapping.')])

    def run(self, paths, patterns):
        for path in paths:
            if not contained(self.root, path):
                self.item(path, '$', None, issues=[('outside-root', 'Symlink target outside source root was not read.')])
                continue
            prov = self.provenance(path)
            self.files.append({k:prov[k] for k in ('path','sha256','blob_oid','working_tree')})
            try:
                if path.suffix in {'.db', '.sqlite', '.sqlite3'}:
                    self.database(path)
                elif path.stat().st_size > MAX_METADATA_BYTES:
                    self.item(path, '$', None, issues=[('large-metadata', 'Metadata exceeds 8 MiB; immutable digest retained; owner must select a smaller export.')])
                elif path.suffix == '.json':
                    self.json_file(path, strict_json(path.read_bytes()))
                elif path.suffix == '.md':
                    self.markdown(path, path.read_bytes().decode('utf-8'))
                else:
                    self.item(path, '$', None, issues=[('unsupported-format', 'Selected file digest preserved; unsupported format.')])
            except (ValueError, TypeError, UnicodeError, sqlite3.Error, OSError) as exc:
                # Keep any preceding rows: remaining container explicitly excepted.
                self.item(path, 'read-error', None, issues=[('read-error', str(exc))])
        if not paths:
            self.item(self.root / '(discovery)', '$', None, issues=[('no-inputs', 'No files matched the documented discovery patterns.')])
        if self.adapter == 'parallax' and not any(p.suffix in {'.db','.sqlite','.sqlite3'} for p in paths):
            self.item(self.root / '(journal)', '$', None, issues=[('journal-unavailable', 'No SQLite journal selected/discovered; no intent/outcome history invented.')])
        # Do not silently merge even byte-identical duplicates from different source locations.
        counts = Counter(r['id'] for r in self.records)
        collisions = {ident for ident,count in counts.items() if count > 1}
        if collisions:
            self.records = [r for r in self.records if r['id'] not in collisions]
            for item in self.inventory:
                if collisions.intersection(item['candidate_ids']):
                    item['candidate_ids'] = [i for i in item['candidate_ids'] if i not in collisions]
                    item['disposition'] = 'exception'
                    item['exceptions'].append(dict(code='identity-collision', message='Repeated namespaced identity requires owner disambiguation; raw records retained.'))
        for path, before in self.before.items():
            if (file_digest(path) if path.exists() else None) != before:
                raise ValueError(f'source changed during import: {path.relative_to(self.root)}')
        # Include external dataset bytes and SQLite sidecars in the immutable file
        # manifest, not only the selected metadata/DB file. Missing files stay in
        # record missingness; they never receive an invented digest.
        selected_files = {f['path'] for f in self.files}
        for path, before in sorted(self.before.items()):
            rel = path.relative_to(self.root).as_posix()
            if before is not None and rel not in selected_files:
                prov = self.provenance(path)
                self.files.append({k:prov[k] for k in ('path','sha256','blob_oid','working_tree')})
        if git(self.root, 'rev-parse', 'HEAD') != self.revision:
            raise ValueError('source Git revision changed during import')
        report = dict(schema_version=1, kind='import-report', adapter=self.adapter, repository=self.repository,
                      source_revision=self.revision, dry_run=True, discovery=patterns, files=self.files,
                      inventory=self.inventory, candidates=self.records,
                      aliases=[dict(alias=a,id=r['id']) for r in self.records for a in r['aliases']],
                      manifest=dict(schema_version=1, kind='manifest', repositories=[dict(id=self.repository,git_revision=self.revision)],
                                    references=[reference(r) for r in self.records]),
                      counts=dict(discovered=len(self.inventory), mapped=sum(i['disposition']=='mapped' for i in self.inventory),
                                  exceptions=sum(i['disposition']=='exception' for i in self.inventory)), source_unchanged=True)
        return report


def import_source(root, adapter, repository, *, selected=None, expected_revision=None):
    root = Path(root).resolve(strict=True)
    if not root.is_dir() or adapter not in PATTERNS:
        raise ValueError('source must be a directory and adapter must be supported')
    if not re.fullmatch(r'[a-z0-9][a-z0-9.-]*', repository):
        raise ValueError('repository identity must be an explicit stable namespace, not a path')
    revision = git(root, 'rev-parse', 'HEAD')
    checkout = git(root, 'rev-parse', '--show-toplevel')
    if checkout and Path(checkout).resolve() != root:
        raise ValueError('Git source root must be the checkout root; use --select for nested inputs')
    if expected_revision and expected_revision != revision:
        raise ValueError(f'source revision mismatch: expected {expected_revision}, inspected {revision}')
    paths, patterns = discover(root, adapter, selected)
    return Importer(root, adapter, repository, revision).run(paths, patterns)


def write_report(report, output, source_roots):
    """Create a new report only; reject source-root, symlink and existing-file targets."""
    output = Path(output)
    resolved = output.resolve()
    if any(resolved.is_relative_to(Path(root).resolve()) for root in source_roots):
        raise ValueError('report output must be outside every source root')
    # Exclusive create also rejects hardlinks and symlinks to existing source files.
    with output.open('x', encoding='utf-8') as f:
        json.dump(report, f, indent=2, ensure_ascii=False, allow_nan=False)
        f.write('\n')
