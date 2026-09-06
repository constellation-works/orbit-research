"""Disposable, fail-closed projection of explicitly supplied owner documents.

No authoring, fetch, importer execution, or operational task state lives here.
Reconciliation is derived separately; original records and verdicts are never edited.
"""
from collections import defaultdict
from contextlib import closing
from copy import deepcopy
import fcntl
import json
import os
from pathlib import Path
import re
import sqlite3
import tempfile

from .contract import _references, _schema_errors, canonical, digest_bytes, validate
from .importers import file_digest, git, strict_json
from .native import Owner, checkout, git_bytes, safe_path
from .science import confirmation_errors, native_reference_errors


class IndexBuildError(ValueError):
    def __init__(self, problems):
        self.problems = problems
        super().__init__('; '.join(f'{p["source"]}: {p["reason"]}' for p in problems))


def pin(ref):
    return (ref['repository'], ref['id'], ref['revision_id'], ref['source_revision'])


def record_pin(record):
    p = record['provenance']
    return p['repository'], record['id'], record['revision_id'], p['git_revision']


def key(value):
    return digest_bytes(canonical(value))[7:]


def load_config(path):
    path = Path(path).resolve(strict=True)
    config = strict_json(path.read_bytes())
    if (not isinstance(config, dict) or config.get('schema_version') != 1 or
            set(config) - {'schema_version', 'checkouts', 'documents', 'media', 'checkout_urls'} or
            not isinstance(config.get('checkouts'), dict) or not isinstance(config.get('documents'), list)):
        raise ValueError('index config requires schema_version 1, checkouts object and documents array')
    roots = {}
    for repository, root in config['checkouts'].items():
        if not isinstance(root, str) or not re.fullmatch(r'[a-z0-9][a-z0-9.-]*', repository):
            raise ValueError('invalid checkout namespace')
        roots[repository] = checkout(path.parent / root)
    if not all(isinstance(p, str) and p for p in config['documents']):
        raise ValueError('documents must be non-empty path strings')
    if not isinstance(config.get('checkout_urls', {}), dict):
        raise ValueError('checkout_urls must be an object')
    if not isinstance(config.get('media', []), list):
        raise ValueError('media must be an array')
    for item in config.get('media', []):
        if not isinstance(item, dict) or not isinstance(item.get('record'), dict):
            raise ValueError('media entries require an exact record reference object')
        manifest = dict(schema_version=1, kind='manifest', repositories=[], references=[item['record']])
        if _schema_errors(manifest, 'manifest'):
            raise ValueError('media record reference is invalid')
        if any(not isinstance(v, str) for k, v in item.items() if k != 'record'):
            raise ValueError('media fields other than record must be strings')
    paths = []
    for relative in config['documents']:
        p = path.parent / relative
        # Operator-selected directories are bounded, nonrecursive record inventories.
        if any(parent.is_symlink() for parent in [p, *p.parents]):
            raise ValueError(f'document symlink is forbidden: {p}')
        if p.is_dir():
            paths.extend(sorted(p.glob('*.json')))
        else:
            paths.append(p)
    if not paths:
        raise ValueError('no owner documents selected')
    return config, roots, sorted(set(paths))


def guard_output(output, roots, inputs=()):
    output = Path(output)
    if any(p.is_symlink() for p in [output, *output.parents]):
        raise ValueError('output may not use symlinks')
    resolved = output.resolve()
    if any(resolved.is_relative_to(root) for root in roots.values()):
        raise ValueError('projection output must be outside every owner checkout')
    if any(resolved == Path(p).resolve() or Path(p).resolve().is_relative_to(resolved) for p in inputs):
        raise ValueError('output overlaps index input')
    return resolved


def _documents(paths):
    records, manifests, inventories, observations, problems = [], [], [], {}, []
    for path in paths:
        try:
            if path.is_symlink():
                raise ValueError('document symlink is forbidden')
            data = path.read_bytes()
            observations[path] = digest_bytes(data)
            doc = strict_json(data)
            kind = doc.get('kind') if isinstance(doc, dict) else None
            if kind == 'manifest':
                errors = validate(doc, _structural=True)
                batch, ms = [], [doc]
            elif kind == 'export':
                errors = _schema_errors(doc, 'export')
                batch, ms = doc.get('records', []), doc.get('manifests', [])
                if not errors and {pin(r) for m in ms for r in m['references']} != {record_pin(r) for r in batch}:
                    errors.append('export manifests do not account for exact record snapshots')
            elif kind == 'import-report':
                errors = validate(doc, _structural=True)
                batch, ms = doc.get('candidates', []), [doc.get('manifest', {})]
                inventories.append(dict(source=str(path), counts=doc.get('counts'), inventory=doc.get('inventory')))
            else:
                errors, batch, ms = [], [doc], []
            for m in ms:
                errors.extend(validate(m, _structural=True))
            for r in batch:
                errors.extend(validate(r, _structural=True))
            if errors:
                raise ValueError('; '.join(errors))
            records.extend((r, str(path)) for r in batch)
            manifests.extend(ms)
        except (OSError, ValueError, TypeError, KeyError) as exc:
            problems.append(dict(source=str(path), reason=str(exc)))
    if problems:
        raise IndexBuildError(problems)
    return records, manifests, inventories, observations


def _source_state(record, roots, cache, observations):
    p = record['provenance']
    root = roots.get(p['repository'])
    if root is None:
        return ['owner checkout is not mapped']
    if not p['git_revision'] or p['working_tree']:
        return ['source is unpinned or marked working-tree; owner reconciliation required']
    identity = (p['repository'], p['git_revision'], p['path'], p['sha256'])
    if identity not in cache:
        reasons = []
        try:
            path = safe_path(root, p['path'])
            raw = git_bytes(root, p['git_revision'], p['path'])
            if digest_bytes(raw) != p['sha256']:
                reasons.append('pinned source bytes do not match provenance SHA-256')
            if p.get('blob_oid') and git(root, 'rev-parse', f'{p["git_revision"]}:{p["path"]}') != p['blob_oid']:
                reasons.append('pinned source blob differs from provenance')
            if not path.is_file():
                reasons.append('source missing from mapped checkout')
            else:
                observations[path] = file_digest(path)
                if observations[path] != p['sha256']:
                    reasons.append('source changed in mapped checkout; exact history retained')
        except (OSError, ValueError) as exc:
            reasons.append(str(exc))
        cache[identity] = reasons
    reasons = list(cache[identity])
    if record.get('schema_version') == 2 and not reasons:
        try:
            snapshot = Owner(root, p['repository'])._snapshot(root, p['path'], p['git_revision'])
            if canonical(snapshot) != canonical(record):
                reasons.append('native export differs from exact owner receipt')
        except (ValueError, OSError) as exc:
            reasons.append('native receipt: ' + str(exc))
    return reasons


def _artifact_state(record, roots, observations):
    p = record['payload']
    if p['availability'] != 'available':
        return [f'artifact is {p["availability"]}']
    locator = p.get('locator')
    root = roots.get(record['provenance']['repository'])
    if not root or not locator:
        return ['artifact bytes are not locally mapped']
    try:
        # Remote URLs and opaque locators stay explicit missingness, never downloads.
        if ':' in locator or '\\' in locator:
            return ['artifact locator requires explicit owner verification; no automatic fetch']
        path = safe_path(root, locator)
        if not path.is_file():
            return ['artifact bytes missing from mapped checkout']
        observations[path] = file_digest(path)
        if observations[path] != p['snapshot_digest']:
            return ['artifact bytes differ from snapshot digest (or require a typed owner verifier)']
    except (OSError, ValueError) as exc:
        return [str(exc)]
    return []


def project(config_path):
    config, roots, paths = load_config(config_path)
    pairs, manifests, inventories, observations = _documents(paths)
    if not manifests:
        raise ValueError('at least one explicit owner manifest is required')
    declared = {pin(ref) for m in manifests for ref in m['references']
                if (ref['repository'], ref['source_revision']) in
                {(p['id'], p['git_revision']) for p in m['repositories']}}
    grouped = defaultdict(dict)
    locations = defaultdict(set)
    for record, source in pairs:
        k = record_pin(record)
        grouped[k][canonical(record)] = record
        locations[k].add(source)
    nodes, cache = {}, {}
    for k in sorted(grouped, key=canonical):
        variants = [grouped[k][v] for v in sorted(grouped[k])]
        r = variants[0]
        reasons = _source_state(r, roots, cache, observations)
        if len(variants) > 1:
            reasons.append('conflicting records share this exact identity/revision/source pin')
        if k not in declared:
            reasons.append('exact record/source pin is absent from supplied manifests')
        if r['kind'] == 'artifact':
            reasons.extend(_artifact_state(r, roots, observations))
        code = r['payload'].get('code') or r['payload'].get('semantic', {}).get('code')
        if code:
            root = roots.get(code['repository'])
            rev = code['git_revision']
            if not root or not re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}', rev) or git(root, 'cat-file', '-t', rev) != 'commit':
                reasons.append('exact code commit is not available in mapped repository')
        reasons.extend('owner missingness: ' + m for m in r['missingness'])
        nodes[k] = dict(key=key(k), pin=list(k), record=r, variants=variants if len(variants) > 1 else [],
                        documents=sorted(locations[k]), reasons=reasons, edges=[], assessments=[])
    # Only explicit supersession can select a head; file order and timestamps cannot.
    by_id = defaultdict(set)
    superseded = defaultdict(set)
    for k, n in nodes.items():
        by_id[k[:2]].add(k[2])
        for ref in n['record'].get('authorship', {}).get('supersedes', []):
            if pin(ref) in nodes and not n['reasons']:
                superseded[(ref['repository'], ref['id'])].add(ref['revision_id'])
    for k, n in nodes.items():
        r = n['record']
        heads = by_id[k[:2]] - superseded[k[:2]]
        n['heads'] = sorted(heads)
        n['history'] = 'historical' if r['provenance']['historical'] else ('superseded' if k[2] not in heads else 'current')
        if not r['provenance']['historical'] and len(heads) > 1:
            n['reasons'].append('conflicting revision heads require an owner decision')
        for ref in _references(r):
            target = nodes.get(pin(ref))
            reason = ('exact target is absent' if not target else '; '.join(target['reasons']))
            if not ref['source_revision']:
                reason = reason or 'target source revision is unpinned'
            n['edges'].append(dict(reference=ref, target=target['key'] if target else None,
                                   status='pending' if reason else 'resolved', reason=reason))
        if r['kind'] == 'assessment':
            target = nodes.get(pin(r['payload']['claim']))
            if target:
                target['assessments'].append(n['key'])
    # Reconcile a private copy for scientific checks; never change canonical ref statuses.
    derived = {k: deepcopy(n['record']) for k, n in nodes.items()}
    for k, r in derived.items():
        for ref, edge in zip(_references(r), nodes[k]['edges']):
            ref['status'] = edge['status']
    known = {(r['id'], r['revision_id'], r['provenance']['git_revision']): r for r in derived.values()}
    for k, n in nodes.items():
        r = derived[k]
        if r.get('schema_version') == 2:
            n['reasons'].extend(native_reference_errors(r, known))
        if r['kind'] == 'assessment' and r['payload']['inference'] == 'confirmatory-primary':
            n['reasons'].extend(confirmation_errors(r, known))
    # Fixed point propagates every unresolved dependency, including cycles, without recursion.
    pending = {k for k, n in nodes.items() if n['reasons'] or any(e['status'] == 'pending' for e in n['edges'])}
    while True:
        more = {k for k, n in nodes.items() if any(pin(e['reference']) in pending for e in n['edges'])}
        if more <= pending:
            break
        pending |= more
    for k, n in nodes.items():
        for edge in n['edges']:
            if pin(edge['reference']) in pending:
                edge.update(status='pending', reason=edge['reason'] or 'target has unresolved dependencies; inspect exact trace')
        n['reconciliation'] = 'pending' if k in pending else 'resolved'
        r, p = n['record'], n['record']['payload']
        n['axes'] = dict(activity=r['activity'], execution=p.get('execution_status', 'not-applicable'),
                         verdict=p.get('verdict', 'not-assessed'), controls=p.get('controls', 'not-applicable'))
        n['confirmation'] = ('eligible' if r['kind'] == 'assessment' and p['inference'] == 'confirmatory-primary'
                             and n['history'] == 'current' and r['activity'] == 'active' and k not in pending else 'not-current')
        if r['kind'] == 'assessment':
            claim = nodes.get(pin(p['claim']))
            if claim and (claim['history'] != 'current' or claim['record']['activity'] != 'active'):
                n['confirmation'] = 'not-current'
                n['reasons'].append('assessment targets a historical, superseded or inactive claim')
        n['reasons'] = sorted(set(n['reasons']))
    # Competing eligible assessments do not silently adjudicate one another.
    for n in nodes.values():
        assessments = [a for a in nodes.values() if a['key'] in n['assessments'] and a['confirmation'] == 'eligible']
        if len({a['axes']['verdict'] for a in assessments}) > 1:
            for a in assessments:
                a['confirmation'] = 'conflicting'
                a['reasons'].append('current assessments disagree on this exact claim pin')
    conflicts = {k for k, n in nodes.items() if n['confirmation'] == 'conflicting'}
    while conflicts:
        for k in conflicts:
            n = nodes[k]
            n['reconciliation'] = 'pending'
            if n['confirmation'] == 'eligible':
                n['confirmation'] = 'not-current'
        dependents = set()
        for k, n in nodes.items():
            for edge in n['edges']:
                if pin(edge['reference']) in conflicts:
                    edge.update(status='pending', reason='target has conflicting assessments in its evidence closure')
                    if n['reconciliation'] != 'pending':
                        dependents.add(k)
        conflicts = dependents
    unresolved = [dict(reference=ref, reason='manifest target is absent', target=None, status='pending')
                  for m in manifests for ref in m['references'] if pin(ref) not in nodes]
    for path, digest in observations.items():
        if file_digest(path) != digest:
            raise IndexBuildError([dict(source=str(path), reason='owner document changed during rebuild')])
    result = dict(schema_version=1, kind='research-index', records=list(nodes.values()),
                  manifests=sorted(manifests, key=canonical), unresolved=sorted(unresolved, key=canonical),
                  inventories=inventories, owners=sorted(roots),
                  notice='Snapshot projection. Rebuild to observe owner changes. Execution success is not scientific support.')
    result['content_digest'] = digest_bytes(canonical(result))
    return result, config, roots, paths


def rebuild(config_path, database):
    config, roots, paths = load_config(config_path)
    output = guard_output(database, roots, [config_path, *paths])
    output.parent.mkdir(parents=True, exist_ok=True)
    lock = os.open(output.parent, os.O_RDONLY | os.O_DIRECTORY)
    temporary = None
    try:
        fcntl.flock(lock, fcntl.LOCK_EX)
        result, config, roots, paths = project(config_path)
        guard_output(output, roots, [config_path, *paths])
        fd, temporary = tempfile.mkstemp(prefix='.research-index-', dir=output.parent)
        os.close(fd)
        with closing(sqlite3.connect(temporary)) as conn, conn:
            conn.executescript('''
                CREATE TABLE projection (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);
                CREATE TABLE records (key TEXT PRIMARY KEY, repository TEXT NOT NULL, id TEXT NOT NULL,
                    revision TEXT NOT NULL, source_revision TEXT, kind TEXT NOT NULL, body TEXT NOT NULL);
                CREATE TABLE links (source TEXT NOT NULL, ordinal INTEGER NOT NULL, target TEXT,
                    status TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(source,ordinal));
                CREATE INDEX record_identity ON records(repository,id,revision,source_revision);
            ''')
            conn.execute('INSERT INTO projection VALUES (1,?)', (canonical(result).decode(),))
            for n in result['records']:
                conn.execute('INSERT INTO records VALUES (?,?,?,?,?,?,?)',
                             (n['key'], *n['pin'], n['record']['kind'], canonical(n).decode()))
                conn.executemany('INSERT INTO links VALUES (?,?,?,?,?)',
                                 [(n['key'], i, e['target'], e['status'], canonical(e).decode()) for i, e in enumerate(n['edges'])])
        with open(temporary, 'rb') as stream:
            os.fsync(stream.fileno())
        os.replace(temporary, output)
        temporary = None
        os.fsync(lock)
    finally:
        if temporary is not None:
            Path(temporary).unlink(missing_ok=True)
        os.close(lock)
    return dict(database=str(output), records=len(result['records']), content_digest=result['content_digest'],
                pending=sum(n['reconciliation'] == 'pending' for n in result['records']))


def read_index(database):
    path = Path(database).resolve(strict=True)
    with closing(sqlite3.connect(path.as_uri() + '?mode=ro', uri=True)) as conn:
        result = strict_json(conn.execute('SELECT body FROM projection WHERE id=1').fetchone()[0])
    digest = result.pop('content_digest')
    if digest != digest_bytes(canonical(result)):
        raise ValueError('projection content digest mismatch; rebuild from owner files')
    result['content_digest'] = digest
    return result


def trace(database, record_key):
    projection = read_index(database)
    nodes = {n['key']: n for n in projection['records']}
    if record_key not in nodes:
        raise ValueError('trace requires an exact indexed snapshot key')
    seen, queue = set(), [record_key]
    while queue:
        k = queue.pop()
        if k in seen:
            continue
        seen.add(k)
        queue.extend(nodes[k]['assessments'])
        queue.extend(e['target'] for e in nodes[k]['edges'] if e['target'])
    return dict(root=record_key, records=[nodes[k] for k in sorted(seen)],
                content_digest=projection['content_digest'])
