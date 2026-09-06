"""Owner-native immutable JSON appends. Git supplies publication pins, Orbit execution."""
from contextlib import contextmanager
from copy import deepcopy
from datetime import datetime, timezone
import fcntl
import os
from pathlib import Path
import re
import subprocess
import tempfile
from urllib.parse import quote

from .contract import (_references, canonical, digest_bytes, make_record, protocol_digest,
                       reference, reconcile, revision_digest, validate)
from .importers import git, strict_json
from .science import instant, native_errors, require

OPERATIONS = {'program': 'program', 'claim': 'claim', 'artifact': 'artifact',
              'preregister': 'protocol', 'begin-run': 'experiment',
              'record-run': 'experiment', 'assess': 'assessment', 'retire': None}


def now():
    return datetime.now(timezone.utc).isoformat()


def checkout(path):
    root = Path(path).resolve(strict=True)
    require(git(root, 'rev-parse', '--show-toplevel') == str(root), 'owner/source root must be an explicit Git checkout root')
    require(git(root, 'rev-parse', 'HEAD') is not None, 'owner requires an initial Git commit')
    return root


def git_bytes(root, revision, path):
    require(re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}', revision) is not None, 'full Git revision required')
    p = subprocess.run(['git', '-C', str(root), 'show', f'{revision}:{path}'], capture_output=True,
                       env={**os.environ, 'GIT_OPTIONAL_LOCKS': '0'})
    require(p.returncode == 0, f'no committed source at {revision}:{path}')
    return p.stdout


def safe_path(root, relative):
    rel = Path(relative)
    require(not rel.is_absolute() and rel.parts and all(p not in {'.git', '.orbit', '..'} for p in rel.parts), 'path must remain inside the scientific owner root')
    path = root / rel
    current = root
    for part in rel.parts:
        current /= part
        require(not current.is_symlink(), 'symlinks are not allowed in canonical paths')
    require(path.resolve().is_relative_to(root), 'path escapes owner root')
    return path


def record_key(record):
    return record['id'], record['revision_id'], record['provenance']['git_revision']


class Owner:
    def __init__(self, root, repository, *, records='research/records', sources=None, artifact_resolver=None):
        require(re.fullmatch(r'[a-z0-9][a-z0-9.-]*', repository) is not None, 'explicit repository namespace required')
        self.root, self.repository = checkout(root), repository
        self.relative = Path(records).as_posix()
        require(self.relative.startswith('research/'), 'canonical records must live under research/ for exact source discovery')
        self.directory = safe_path(self.root, records)
        self.sources = {repository: self.root}
        self.artifact_resolver = artifact_resolver
        for name, path in (sources or {}).items():
            require(name != repository, 'cannot override owner routing')
            self.sources[name] = checkout(path)

    @contextmanager
    def locked(self):
        # Lock the directory inode: no mutable database, index or persistent lock file.
        self.directory.mkdir(parents=True, exist_ok=True)
        fd = os.open(self.directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX)
            yield
        finally:
            os.close(fd)

    def entries(self):
        rows, previous = [], None
        committed = git(self.root, 'ls-tree', '-r', '--name-only', 'HEAD', '--', self.relative) or ''
        for relative in committed.splitlines():
            if re.fullmatch(r'[0-9]{8}-[0-9a-f]{64}\.json', Path(relative).name):
                require(safe_path(self.root, relative).is_file(), 'committed append was removed; restore owner history before writing')
        for path in sorted(self.directory.glob('*.json')):
            require(not path.is_symlink(), 'canonical record cannot be a symlink')
            data = path.read_bytes()
            r = strict_json(data)
            require(isinstance(r, dict), 'invalid canonical record')
            errors = validate(r, _structural=True)
            require(not errors, 'invalid canonical record: ' + '; '.join(errors))
            require(r.get('schema_version') == 2 and r['provenance']['repository'] == self.repository, 'wrong native schema/owner')
            a = r['authorship']
            digest = digest_bytes(canonical(r))
            require(path.name == f'{len(rows)+1:08d}-{digest[7:]}.json', 'record filename/content/sequence mismatch')
            require(a['sequence'] == len(rows)+1 and a['previous'] == previous, 'append chain is incomplete or forked')
            require(not native_errors(r), 'invalid native invariants: ' + '; '.join(native_errors(r)))
            if rows:
                require(instant(a['registered_at']) >= instant(rows[-1][1]['authorship']['registered_at']), 'registration clock regressed')
            rows.append((path, r))
            previous = digest
        require(len({r['authorship']['request_id'] for _, r in rows}) == len(rows), 'duplicate idempotency key')
        return rows

    def records(self):
        return [r for _, r in self.entries()]

    def heads(self, ident):
        records = [r for r in self.records() if r['id'] == ident]
        superseded = {x['revision_id'] for r in records for x in r['authorship']['supersedes'] if x['id'] == ident}
        return sorted(r['revision_id'] for r in records if r['revision_id'] not in superseded)

    def pin(self, ident, revision, source_revision):
        matches = [(p, r) for p, r in self.entries() if r['id'] == ident and r['revision_id'] == revision]
        require(len(matches) == 1, 'exact identity/revision is missing or ambiguous')
        path, record = matches[0]
        return self._snapshot(self.root, path.relative_to(self.root).as_posix(), source_revision, expected=record)

    def _snapshot(self, root, path, revision, expected=None):
        safe_path(root, path)
        data = git_bytes(root, revision, path)
        record = strict_json(data)
        require(not validate(record, _structural=True), 'invalid pinned record')
        if record.get('schema_version') == 2:
            current, current_path = record, Path(path)
            while True:
                sequence = current['authorship']['sequence']
                require(current_path.name == f'{sequence:08d}-{digest_bytes(canonical(current))[7:]}.json',
                        'pinned native receipt filename/content mismatch')
                previous = current['authorship']['previous']
                if sequence == 1:
                    require(previous is None, 'first native receipt cannot have a predecessor')
                    break
                require(previous is not None, 'native receipt chain is missing a predecessor')
                current_path = current_path.parent / f'{sequence-1:08d}-{previous[7:]}.json'
                parent = strict_json(git_bytes(root, revision, current_path.as_posix()))
                require(not validate(parent, _structural=True) and parent.get('schema_version') == 2,
                        'invalid pinned native predecessor')
                require(parent['authorship']['sequence'] == sequence-1 and parent['provenance']['repository'] == current['provenance']['repository'],
                        'pinned native predecessor sequence/owner mismatch')
                require(instant(parent['authorship']['registered_at']) <= instant(current['authorship']['registered_at']),
                        'pinned registration clock regressed')
                current = parent
        if expected is not None:
            require(canonical(record) == canonical(expected), 'committed record differs from canonical append')
        record['provenance'].update(git_revision=revision, blob_oid=git(root, 'rev-parse', f'{revision}:{path}'),
                                    sha256=digest_bytes(data), path=path, selector='$', working_tree=False)
        return record

    def resolve(self, ref):
        """Read only the explicitly routed owner and exact Git snapshot; never HEAD fallback."""
        if self.artifact_resolver is not None:
            from .artifacts import VerifiedArtifact
            proof = self.artifact_resolver(ref)
            if proof is not None:
                require(isinstance(proof, VerifiedArtifact), 'artifact resolver must return a verified capability')
                record = proof.verify()
                require(reference(record)['repository'] == ref['repository'] and record_key(record) == (ref['id'], ref['revision_id'], ref['source_revision']),
                        'artifact resolver returned a different exact pin')
                return record
        root = self.sources.get(ref['repository'])
        require(root is not None and ref['source_revision'], f'unrouted or unpinned reference: {ref["id"]}')
        # A rebuildable search over JSON in the specified snapshot, no secondary authority.
        listing = git(root, 'ls-tree', '-r', '--name-only', ref['source_revision'])
        require(listing is not None, 'source revision unavailable')
        matches = []
        for path in listing.splitlines():
            if not path.endswith('.json') or not path.startswith('research/'):
                continue
            try:
                raw = strict_json(git_bytes(root, ref['source_revision'], path))
            except (ValueError, UnicodeError):
                continue
            if isinstance(raw, dict) and raw.get('id') == ref['id'] and raw.get('revision_id') == ref['revision_id']:
                matches.append(self._snapshot(root, path, ref['source_revision']))
        require(len(matches) == 1, f'exact reference missing or ambiguous: {ref["id"]}')
        r = matches[0]
        require(r['provenance']['repository'] == ref['repository'], 'source namespace mismatch')
        return r

    def closure(self, records):
        known = {record_key(r): r for r in records}
        pending, queue = [], list(records)
        while queue:
            r = queue.pop()
            for ref in _references(r):
                key = (ref['id'], ref['revision_id'], ref['source_revision'])
                if key in known:
                    continue
                try:
                    target = self.resolve(ref)
                except ValueError as exc:
                    if ref['status'] == 'resolved':
                        raise
                    pending.append(dict(reference=ref, reason=str(exc)))
                    continue
                known[key] = target
                queue.append(target)
        return list(known.values()), pending

    def apply(self, operation, request):
        require(operation in OPERATIONS and isinstance(request, dict), 'unknown operation or invalid request')
        allowed = {'request_id', 'id', 'scope', 'payload', 'orbit_links', 'expected_heads',
                   'supersedes', 'reason', 'references', 'presentation', 'limitations', 'kind'}
        require(not set(request) - allowed, 'unknown request fields: ' + ', '.join(sorted(set(request) - allowed)))
        for field in ('request_id', 'id', 'scope', 'payload', 'orbit_links', 'expected_heads', 'reason'):
            require(field in request, f'request requires {field}')
        require(isinstance(request['request_id'], str) and request['request_id'].strip(), 'idempotency key required')
        require(isinstance(request['expected_heads'], list), 'expected_heads must explicitly list all current revisions')
        req_digest = digest_bytes(canonical(dict(operation=operation, request=request)))
        with self.locked():
            entries = self.entries()
            for _, r in entries:
                if r['authorship']['request_id'] == request['request_id']:
                    require(r['authorship']['request_digest'] == req_digest, 'idempotency key reused for different request')
                    return r
            kind = OPERATIONS[operation] or request.get('kind')
            require(kind in {'program', 'claim', 'artifact', 'protocol', 'experiment', 'assessment'}, 'invalid kind')
            ident = f'urn:research:{self.repository}:{kind}:{quote(str(request["id"]), safe="")}'
            previous_records = [r for _, r in entries if r['id'] == ident]
            heads = self.heads(ident)
            require(sorted(request['expected_heads']) == heads, f'stale base: expected {request["expected_heads"]}, actual {heads}')
            bases = request.get('supersedes', heads)
            require(isinstance(bases, list) and len(set(bases)) == len(bases) and set(bases) <= set(heads), 'supersedes must select current heads; explicitly retain other conflicts')
            parents = [r for r in previous_records if r['revision_id'] in bases]
            parent_refs = []
            for parent in parents:
                try:
                    pinned = self.pin(parent['id'], parent['revision_id'], git(self.root, 'rev-parse', 'HEAD'))
                    parent_refs.append(reference(pinned, 'resolved'))
                except ValueError:
                    parent_refs.append(reference(parent))
            timestamp = now()
            if entries:
                require(instant(timestamp) >= instant(entries[-1][1]['authorship']['registered_at']), 'registration clock regressed')
            payload = deepcopy(request['payload'])
            if operation == 'preregister':
                require(set(payload) == {'semantic'}, 'preregister accepts semantic content only; no caller freeze dates or history')
                payload.update(semantic_digest=protocol_digest(payload['semantic']), freeze='registered', frozen_at=timestamp, freeze_evidence=None)
                sem = payload['semantic']
                source = self.sources.get(sem['code']['repository'])
                require(source is not None and git(source, 'rev-parse', '--verify', sem['code']['git_revision']+'^{commit}') == sem['code']['git_revision'],
                        'code revision must exist in an explicitly routed source checkout')
                inputs = [self.resolve(ref) for ref in sem['inputs']]
                require(any(r['kind'] == 'artifact' and r['payload']['snapshot_digest'] == sem['holdout']['digest'] for r in inputs),
                        'holdout or seed-plan digest must be an exact frozen input artifact')
            if operation in {'begin-run', 'record-run'}:
                forbidden = {'started_at', 'finished_at'}
                require(not forbidden.intersection(payload), 'run timestamps are observed, never supplied')
                if operation == 'begin-run':
                    require(not previous_records and payload['execution_status'] == 'running' and payload['start'] is None and not payload['result_artifacts'], 'begin-run needs a new running identity with no results')
                    protocol = self.resolve(payload['protocol'])
                    require(protocol['kind'] == 'protocol' and protocol.get('schema_version') == 2, 'begin-run requires native frozen protocol')
                    protocol_root = self.sources[payload['protocol']['repository']]
                    require(git(protocol_root, 'merge-base', '--is-ancestor', payload['protocol']['source_revision'], 'HEAD') is not None,
                            'freeze commit must precede run registration')
                    sem = protocol['payload']['semantic']
                    require(payload['code'] == sem['code'] and payload['inputs'] == sem['inputs'] and payload['holdout_digest'] == sem['holdout']['digest'], 'start must bind exact frozen code/data/holdout')
                    require(instant(timestamp) >= instant(sem['holdout']['evaluation_not_before']), 'evaluation boundary has not arrived')
                    payload.update(started_at=timestamp, finished_at=None)
                else:
                    require(payload['execution_status'] in {'completed', 'failed', 'cancelled'}, 'record-run requires terminal execution status')
                    start = self.resolve(payload['start']) if payload['start'] else None
                    if start:
                        require(start['id'] == ident and start['kind'] == 'experiment' and start['payload']['execution_status'] == 'running', 'start receipt must be for this run')
                        require(request['orbit_links'] == start['orbit_links'], 'run must retain exact starting Orbit provenance')
                        for key in ('protocol', 'code', 'inputs', 'holdout_digest'):
                            require(payload[key] == start['payload'][key] or payload['deviations'], f'run {key} differs from start; explicit deviations required')
                    payload.update(started_at=start['payload']['started_at'] if start else None, finished_at=timestamp)
            if operation == 'retire':
                require(kind in {'program', 'claim'} and len(parents) == 1 and payload == parents[0]['payload'], 'retirement preserves one program/claim revision payload; assessments remain intact')
            provenance = dict(repository=self.repository, git_revision=git(self.root, 'rev-parse', 'HEAD'),
                              blob_oid=None, sha256=req_digest, path=self.relative, selector='$request', historical=False, working_tree=True)
            r = make_record(self.repository, kind, request['id'], payload, provenance,
                            activity='retired' if operation == 'retire' else 'active', scope=request['scope'],
                            limitations=request.get('limitations', []))
            r.update(schema_version=2, orbit_links=deepcopy(request['orbit_links']),
                     references=deepcopy(request.get('references', [])), presentation=deepcopy(request.get('presentation', {})),
                     authorship=dict(registered_at=timestamp, sequence=len(entries)+1,
                                     previous=digest_bytes(canonical(entries[-1][1])) if entries else None,
                                     request_id=request['request_id'], request_digest=req_digest,
                                     supersedes=parent_refs, reason=request['reason']))
            # Supersedes is an owner-local revision link; exact publication pins are optional.
            r['revision_id'] = revision_digest(r)
            require(not any(p['revision_id'] == r['revision_id'] for p in previous_records), 'revision already frozen; presentation changes use linked prose or a new normative revision')
            errors = validate(r, _structural=True) + native_errors(r)
            require(not errors, '; '.join(errors))
            targets, _ = self.closure([r])
            errors = validate(r, targets=[t for t in targets if record_key(t) != record_key(r)], artifact_resolver=self.artifact_resolver)
            require(not errors, '; '.join(errors))
            self._publish(r)
            return r

    def _publish(self, record):
        data = canonical(record) + b'\n'
        digest = digest_bytes(canonical(record))[7:]
        destination = self.directory / f'{record["authorship"]["sequence"]:08d}-{digest}.json'
        fd, name = tempfile.mkstemp(prefix='.append-', dir=self.directory)
        try:
            with os.fdopen(fd, 'wb') as f:
                f.write(data)
                f.flush()
                os.fsync(f.fileno())
            # Hard-link publication is atomic and never replaces an existing path.
            os.link(name, destination)
            directory_fd = os.open(self.directory, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory_fd)
            finally:
                os.close(directory_fd)
        finally:
            os.unlink(name)

    def trace(self, ident, revision):
        matches = [r for r in self.records() if r['id'] == ident and r['revision_id'] == revision]
        require(len(matches) == 1, 'trace requires an exact identity/revision')
        records, pending = self.closure(matches)
        # Include every assessment of this revision, preserving disagreements and retirement.
        assessments = [r for r in self.records() if r['kind'] == 'assessment' and
                       r['payload']['claim']['id'] == ident and r['payload']['claim']['revision_id'] == revision]
        closure, more = self.closure(assessments)
        merged = {record_key(r): r for r in records + closure}
        return dict(schema_version=2, kind='trace', root={'id': ident, 'revision_id': revision},
                    heads=self.heads(ident), records=list(merged.values()), unresolved=pending + more)

    def export(self, revision):
        records = [self.pin(r['id'], r['revision_id'], revision) for r in self.records()]
        require(records, 'no native records to export')
        closure, pending = self.closure(records)
        for record in closure:
            errors = validate(record, targets=[r for r in closure if record_key(r) != record_key(record)], artifact_resolver=self.artifact_resolver)
            require(not errors, 'export validation: ' + '; '.join(errors))
        # v1 manifests intentionally allow one source revision per repository. Group
        # exact historical snapshots instead of replacing all links with latest HEAD.
        manifests = []
        for repository, pin in sorted({(r['provenance']['repository'], r['provenance']['git_revision']) for r in closure}, key=lambda p: (p[0], p[1] or '')):
            subset = [r for r in closure if r['provenance']['repository'] == repository and r['provenance']['git_revision'] == pin]
            manifest = dict(schema_version=1, kind='manifest', repositories=[dict(id=repository, git_revision=pin)], references=[reference(r) for r in subset])
            manifest = reconcile(manifest, closure, artifact_resolver=self.artifact_resolver)
            require(not validate(manifest, targets=closure, artifact_resolver=self.artifact_resolver), 'invalid export manifest')
            manifests.append(manifest)
        return dict(schema_version=2, kind='export', records=closure, manifests=manifests, unresolved=pending)


def write_json_new(document, path):
    """Publish one complete export atomically; no truncation of an existing destination."""
    path = Path(path)
    fd, temporary = tempfile.mkstemp(prefix='.research-export-', dir=path.parent)
    try:
        with os.fdopen(fd, 'wb') as f:
            f.write(canonical(document) + b'\n')
            f.flush()
            os.fsync(f.fileno())
        os.link(temporary, path)
    finally:
        os.unlink(temporary)
