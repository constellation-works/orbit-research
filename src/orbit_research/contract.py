"""Version 1 structural and scientific invariants. No operational state access."""
from __future__ import annotations

from copy import deepcopy
from hashlib import sha256
from importlib.resources import files
from functools import lru_cache
import json
from urllib.parse import quote

from jsonschema import Draft202012Validator
from referencing import Registry, Resource


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False,
                      allow_nan=False).encode('utf-8')


def digest_bytes(data):
    return 'sha256:' + sha256(data).hexdigest()


def protocol_digest(semantic):
    """Only normative semantic content; never presentation prose or source locations."""
    return digest_bytes(canonical(semantic))


def revision_digest(record):
    if record['kind'] == 'protocol':
        return protocol_digest(record['payload']['semantic'])
    return digest_bytes(canonical({k: v for k, v in record.items()
                                   if k not in {'revision_id', 'presentation', 'provenance'}}))


def make_record(repository, kind, legacy_id, payload, provenance, *, legacy=None,
                activity='unknown', scope='unknown', limitations=(), missingness=()):
    """Construct a candidate. Explicit repository namespace, never a source path identity."""
    missingness = list(missingness)
    for field, value in [('activity', activity), ('scope', scope),
                         ('git-revision', provenance['git_revision'])]:
        if value in {'unknown', None} and field not in missingness:
            missingness.append(field)
    record = dict(schema_version=1, kind=kind,
                  id=f'urn:research:{repository}:{kind}:{quote(str(legacy_id), safe="")}',
                  aliases=[str(legacy_id)], activity=activity, scope=scope,
                  provenance=deepcopy(provenance), limitations=list(limitations),
                  missingness=list(missingness), legacy=deepcopy(legacy), references=[],
                  presentation={}, payload=deepcopy(payload))
    record['revision_id'] = revision_digest(record)
    return record


def reference(record, status='pending'):
    return dict(repository=record['provenance']['repository'], id=record['id'],
                revision_id=record['revision_id'],
                source_revision=record['provenance']['git_revision'], status=status)


def _references(record):
    yield from record['references']
    p = record['payload']
    if record['kind'] == 'assessment':
        yield p['claim']
        yield from p['evidence']
    elif record['kind'] == 'experiment':
        if p['protocol']:
            yield p['protocol']
        yield from p['result_artifacts']
    elif record['kind'] == 'protocol' and p['freeze_evidence']:
        yield p['freeze_evidence']


@lru_cache(maxsize=1)
def _schemas():
    return {p.name.removesuffix('.schema.json'): json.loads(p.read_text())
            for p in files('orbit_research').joinpath('schemas/v1').iterdir()
            if p.name.endswith('.schema.json')}


def _schema_errors(value, name):
    schemas = _schemas()
    registry = Registry().with_resources((s['$id'], Resource.from_contents(s)) for s in schemas.values())
    schema = schemas[name]
    return [f'{"/".join(map(str, e.absolute_path)) or "$"}: {e.message}'
            for e in Draft202012Validator(schema, registry=registry).iter_errors(value)]


def validate(document, *, targets=()):
    """Return errors; unresolved references are valid pending data, never current evidence."""
    if not isinstance(document, dict):
        return ['$: expected an object']
    kind = document.get('kind')
    name = kind if kind in {'manifest', 'import-report'} else 'record'
    errors = _schema_errors(document, name)
    if errors:
        return errors
    records = document['candidates'] if kind == 'import-report' else ([] if kind == 'manifest' else [document])
    known = {}
    for target in targets:
        # Context is independently validated before it can establish an exact pin.
        if not isinstance(target, dict) or target.get('kind') in {'manifest', 'import-report'}:
            errors.append('target must be an individual scientific record')
            continue
        target_errors = validate(target)
        if target_errors:
            errors.extend('target: ' + e for e in target_errors)
        else:
            key = (target['id'], target['revision_id'])
            if key in known:
                errors.append(f'ambiguous target: {key}')
            known[key] = target
    aliases = {}
    for r in records:
        key = (r['id'], r['revision_id'])
        if key in known:
            errors.append(f'duplicate identity/revision: {key}')
        known[key] = r
        namespace = r['id'].split(':')[2]
        if namespace != r['provenance']['repository'] or r['id'].split(':')[3] != r['kind']:
            errors.append(f'{r["id"]}: identity does not match repository/kind')
        if r['revision_id'] != revision_digest(r):
            errors.append(f'{r["id"]}: revision digest does not match semantic content')
        for alias in r['aliases']:
            alias_key = (namespace, r['kind'], alias)
            if alias_key in aliases and aliases[alias_key] != r['id']:
                errors.append(f'ambiguous alias: {alias_key}')
            aliases[alias_key] = r['id']
        p = r['payload']
        if r['kind'] == 'protocol':
            if p['semantic_digest'] != protocol_digest(p['semantic']):
                errors.append('protocol semantic digest mismatch')
            if r['provenance']['historical'] and p['freeze'] != 'historical-unverified':
                errors.append('historical import cannot fabricate prospective preregistration')
            if p['freeze'] == 'prospective' and (not p['frozen_at'] or not p['freeze_evidence']):
                errors.append('prospective freeze requires timestamp and immutable evidence')
            if p['freeze'] == 'historical-unverified' and (p['frozen_at'] or p['freeze_evidence']):
                errors.append('unverified historical freeze cannot assert verified freeze evidence')
        if r['kind'] == 'artifact' and p['availability'] == 'available' and not p['snapshot_digest']:
            errors.append('available artifact requires immutable snapshot digest')
        if r['kind'] == 'assessment':
            if p['basis'] == 'execution-only' and p['verdict'] in {'supported', 'refuted'}:
                errors.append('execution success/failure is not scientific support/refutation')
            if p['inference'] == 'confirmatory-primary':
                if p['controls'] not in {'passed', 'not-applicable'}:
                    errors.append('confirmatory primary inference requires passing controls')
                if p['basis'] != 'scientific-evidence' or not p['evidence']:
                    errors.append('confirmatory inference requires scientific evidence')
                if r['scope'] == 'unknown':
                    errors.append('confirmatory inference requires explicit scope')
                if any(ref['status'] != 'resolved' for ref in _references(r)):
                    errors.append('pending references cannot be current confirmatory evidence')
            if r['provenance']['historical']:
                if p['inference'] != 'historical' or p['basis'] != 'legacy-report':
                    errors.append('historical assessment must remain a legacy report')
                expected = {'supported':'supported', 'refuted':'refuted', 'mixed':'inconclusive',
                            'inconclusive':'inconclusive', 'conditional':'conditional',
                            'untested':'untested', 'conjecture':'untested'}.get(p['legacy_verdict'], 'unknown')
                if p['verdict'] != expected:
                    errors.append('historical verdict was strengthened or changed')
                if isinstance(r['legacy'], dict) and r['legacy'].get('status') != p['legacy_verdict']:
                    errors.append('legacy verdict differs from retained source status')
    refs = document['references'] if kind == 'manifest' else [ref for r in records for ref in _references(r)]
    if kind == 'import-report':
        refs += document['manifest']['references']
        inv = document['inventory']
        counts = {'discovered': len(inv), 'mapped': sum(i['disposition'] == 'mapped' for i in inv),
                  'exceptions': sum(i['disposition'] == 'exception' for i in inv)}
        if document['counts'] != counts or len({i['key'] for i in inv}) != len(inv):
            errors.append('inventory accounting mismatch or duplicate selector')
        ids = {r['id'] for r in records}
        if any(not r['provenance']['historical'] for r in records):
            errors.append('import candidates must be marked historical')
        if any(r['provenance']['repository'] != document['repository'] or
               r['provenance']['git_revision'] != document['source_revision'] for r in records):
            errors.append('candidate provenance differs from import source pin')
        file_pins = {(f['path'], f['sha256']) for f in document['files']}
        if any((r['provenance']['path'], r['provenance']['sha256']) not in file_pins for r in records):
            errors.append('candidate source bytes are absent from file manifest')
        accounted = set()
        for i in inv:
            accounted.update(i['candidate_ids'])
            if i['disposition'] == 'exception' and not i['exceptions']:
                errors.append(f'{i["key"]}: exception requires a reason')
            if i['disposition'] == 'mapped' and (not i['candidate_ids'] or i['exceptions']):
                errors.append(f'{i["key"]}: mapped item requires candidates and no exceptions')
        if accounted != ids:
            errors.append('inventory must account for every candidate and reference only candidates')
        expected_aliases = sorted((a, r['id']) for r in records for a in r['aliases'])
        if sorted((a['alias'], a['id']) for a in document['aliases']) != expected_aliases:
            errors.append('alias report mismatch')
    manifest = document if kind == 'manifest' else document.get('manifest')
    if manifest:
        pins = {(p['id'], p['git_revision']) for p in manifest['repositories']}
        if len({p['id'] for p in manifest['repositories']}) != len(manifest['repositories']):
            errors.append('manifest repository identity has ambiguous source revisions')
        for ref in manifest['references']:
            if ref['status'] == 'resolved' and (ref['repository'], ref['source_revision']) not in pins:
                errors.append('resolved reference is not pinned by manifest repositories')
    for ref in refs:
        if ref['id'].split(':')[2] != ref['repository']:
            errors.append('reference identity does not match repository')
        if ref['status'] == 'resolved':
            target = known.get((ref['id'], ref['revision_id']))
            if not target or target['provenance']['repository'] != ref['repository'] or not ref['source_revision'] or target['provenance']['git_revision'] != ref['source_revision'] or target['provenance']['working_tree']:
                errors.append(f'{ref["id"]}: resolved reference lacks exact validated target/source pin')
            elif target['kind'] == 'experiment' and target['payload']['controls'] == 'failed':
                for r in records:
                    if r['kind'] == 'assessment' and r['payload']['inference'] == 'confirmatory-primary' and ref in r['payload']['evidence']:
                        errors.append('failed-control experiment cannot support primary confirmation')
    for r in records:
        if r['kind'] != 'assessment' or r['payload']['inference'] != 'confirmatory-primary':
            continue
        p = r['payload']
        claim_ref = p['claim']
        claim = known.get((claim_ref['id'], claim_ref['revision_id']))
        if claim and claim['kind'] != 'claim':
            errors.append('assessment must target a claim')
        if claim and claim['kind'] == 'claim' and claim['payload']['domain'] == 'nature':
            if r['scope'] in {'derivation', 'simulation-under-assumptions', 'synthetic-calibration'}:
                errors.append('model or synthetic scope cannot confirm a claim about nature')
            for ref in p['evidence']:
                evidence = known.get((ref['id'], ref['revision_id']))
                if evidence and evidence['scope'] in {'derivation', 'simulation-under-assumptions', 'synthetic-calibration'}:
                    errors.append('model or synthetic evidence cannot confirm a claim about nature')
    return errors


def reconcile(manifest, records):
    """Return a copy with only exact, independently valid pins resolved; no file/network lookup."""
    errors = _schema_errors(manifest, 'manifest')
    if errors:
        raise ValueError('; '.join(errors))
    result = deepcopy(manifest)
    valid = {}
    duplicates = set()
    for record in records:
        if validate(record):
            continue
        key = (record['provenance']['repository'], record['id'], record['revision_id'],
               record['provenance']['git_revision'])
        if key in valid:
            duplicates.add(key)
        valid[key] = record
    pins = {(p['id'], p['git_revision']) for p in manifest['repositories']}
    for ref in result['references']:
        key = (ref['repository'], ref['id'], ref['revision_id'], ref['source_revision'])
        ref['status'] = 'resolved' if (key in valid and key not in duplicates and ref['source_revision']
                                      and not valid[key]['provenance']['working_tree']
                                      and (ref['repository'], ref['source_revision']) in pins) else 'pending'
    return result
