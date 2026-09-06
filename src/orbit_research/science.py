"""Native scientific guards, independent of storage and execution engines."""
from datetime import datetime
import math
import re


def instant(value):
    if not isinstance(value, str):
        raise ValueError('timestamp must be an ISO 8601 string with timezone')
    result = datetime.fromisoformat(value.replace('Z', '+00:00'))
    if result.tzinfo is None:
        raise ValueError('timestamp requires timezone')
    return result


def require(ok, message):
    if not ok:
        raise ValueError(message)


def number(value):
    return type(value) in (int, float) and math.isfinite(value)


def protocol_errors(semantic):
    """Explicit applicability, finite budgets and attainable decision arithmetic."""
    errors = []
    try:
        for field in ('question', 'assumptions', 'analysis', 'exclusions', 'stopping_rule'):
            require(isinstance(semantic.get(field), str) and semantic[field].strip(), f'protocol requires {field}')
        require(bool(semantic.get('claims')), 'protocol requires exact claim references')
        code = semantic['code']
        require(bool(code['repository']) and re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}', code['git_revision']), 'protocol requires exact code revision')
        require(isinstance(semantic['inputs'], list), 'protocol inputs must be explicit')
        design = semantic['design']
        require(design['kind'] in {'empirical', 'synthetic', 'deterministic'}, 'unsupported design kind')
        budget = design['resource_budget']
        require(number(budget['planned']) and number(budget['limit']) and 0 <= budget['planned'] <= budget['limit'] and bool(budget['unit']), 'incoherent resource budget')
        if design['kind'] == 'deterministic':
            convergence = design['convergence']
            require(number(convergence['tolerance']) and convergence['tolerance'] > 0 and
                    type(convergence['max_steps']) is int and convergence['max_steps'] > 0 and
                    bool(convergence['criterion']), 'deterministic work requires convergence criterion, tolerance and step budget')
            require(bool(design['decision']), 'deterministic decision rule required')
        else:
            groups = design['samples']['groups']
            require(bool(groups) and all(type(g['count']) is int and g['count'] > 0 and g['name'] for g in groups), 'positive sample counts required')
            total = sum(g['count'] for g in groups)
            require(type(design['samples']['total']) is int and total == design['samples']['total'], 'sample total does not match enumerated groups')
            require(len({g['name'] for g in groups}) == len(groups), 'duplicate sample group')
            require(bool(design['controls']) and bool(design['baseline']), 'empirical/synthetic design requires controls and baseline')
            rule = design['decision']
            require(all(number(rule[k]) for k in ('threshold', 'attainable_min', 'attainable_max')), 'decision range must be finite')
            require(rule['attainable_min'] <= rule['threshold'] <= rule['attainable_max'], 'decision threshold is unattainable')
            require(rule['operator'] in {'>=', '<=', '>', '<'} and bool(rule['metric']), 'explicit decision metric/operator required')
            require(not (rule['operator'] == '>' and rule['threshold'] == rule['attainable_max']) and
                    not (rule['operator'] == '<' and rule['threshold'] == rule['attainable_min']), 'strict threshold is unattainable')
            if 'binomial_lower_bound' in rule:
                b = rule['binomial_lower_bound']
                require(type(b['n']) is int and 0 < b['n'] <= total and number(b['confidence']) and 0 < b['confidence'] < 1,
                        'invalid binomial sample/confidence')
                # Exact one-sided all-success Clopper-Pearson bound: best possible result.
                best = (1 - b['confidence']) ** (1 / b['n'])
                require(rule['operator'] in {'>=', '>'} and (best > rule['threshold'] if rule['operator'] == '>' else best >= rule['threshold']), 'binomial lower-bound threshold unattainable even with all successes')
        holdout = semantic['holdout']
        require(re.fullmatch(r'sha256:[0-9a-f]{64}', holdout['digest']) is not None, 'immutable holdout/seed-plan digest required')
        require(bool(holdout['policy']), 'holdout access policy required')
        require(instant(holdout['information_cutoff']) <= instant(holdout['evaluation_not_before']), 'holdout cutoff after evaluation boundary')
    except (KeyError, TypeError, ValueError) as exc:
        errors.append('protocol: ' + str(exc))
    return errors


def native_errors(record):
    errors = []
    try:
        a, p = record['authorship'], record['payload']
        registered = instant(a['registered_at'])
        require(not record['provenance']['historical'] and record['legacy'] is None, 'native authoring cannot relabel imported history')
        require(all(link.get('run') for link in record['orbit_links']), 'native provenance requires host/workspace/task/run')
        if record['kind'] == 'protocol':
            errors.extend(protocol_errors(p['semantic']))
            require(p['freeze'] == 'registered' and p['frozen_at'] == a['registered_at'] and p['freeze_evidence'] is None,
                    'native freeze is measured registration, not an asserted prospective timestamp')
            require(instant(p['semantic']['holdout']['information_cutoff']) <= registered <=
                    instant(p['semantic']['holdout']['evaluation_not_before']), 'freeze must precede the evaluation boundary and follow information cutoff')
        elif record['kind'] == 'experiment':
            if p['started_at']:
                require(instant(p['started_at']) <= registered, 'run start after registration')
            if p['finished_at']:
                require(p['started_at'] is None or instant(p['started_at']) <= instant(p['finished_at']), 'finish before start')
                require(instant(p['finished_at']) == registered, 'finish must be measured at registration')
            if p['execution_status'] == 'running':
                require(p['started_at'] == a['registered_at'] and not p['finished_at'] and not p['start'], 'invalid run-start receipt')
        elif record['kind'] == 'assessment':
            if p['evidence_summary'] in {'mixed', 'inconclusive', 'unmeasured'}:
                require(p['verdict'] not in {'supported', 'refuted'}, 'mixed or unmeasured evidence requires a limited verdict')
    except (KeyError, TypeError, ValueError) as exc:
        errors.append('native: ' + str(exc))
    return errors


def native_reference_errors(record, known):
    """Typed native edges and the model/nature boundary apply to exploratory work too."""
    from .contract import _references
    errors = []
    def get(ref):
        return known.get((ref['id'], ref['revision_id'], ref['source_revision']))
    p = record['payload']
    edges = []
    if record['kind'] == 'protocol':
        edges += [(r, 'claim') for r in p['semantic']['claims']]
        edges += [(r, 'artifact') for r in p['semantic']['inputs']]
    elif record['kind'] == 'experiment':
        if p['protocol']:
            edges.append((p['protocol'], 'protocol'))
        edges += [(r, 'artifact') for r in p['inputs'] + p['result_artifacts']]
        for ref in p['result_artifacts']:
            target = get(ref)
            if target and target['kind'] == 'artifact' and target['payload']['role'] != 'result':
                errors.append('run output must reference a result artifact')
    elif record['kind'] == 'assessment':
        edges.append((p['claim'], 'claim'))
        claim = get(p['claim'])
        if claim and claim['kind'] == 'claim' and claim['payload']['domain'] == 'nature' and p['verdict'] in {'supported', 'refuted'}:
            seen, queue = set(), [record]
            while queue:
                r = queue.pop()
                key = (r['id'], r['revision_id'])
                if key in seen:
                    continue
                seen.add(key)
                if r['scope'] in {'derivation', 'simulation-under-assumptions', 'synthetic-calibration'}:
                    errors.append('model or synthetic scope cannot establish a verdict about nature')
                queue.extend(t for ref in _references(r) if (t := get(ref)) is not None)
    for ref, kind in edges:
        target = get(ref)
        if target and target['kind'] != kind:
            errors.append(f'native reference requires {kind}, found {target["kind"]}')
    return errors


def confirmation_errors(assessment, known, *, verified_artifacts=()):
    """Every dependency, including indirect controls, must be usable; no latest-wins."""
    from .contract import _references
    errors, visited = [], set()

    def target(ref):
        key = (ref['id'], ref['revision_id'], ref['source_revision'])
        r = known.get(key)
        if ref['status'] != 'resolved' or not r or (key not in verified_artifacts and (r['provenance']['working_tree'] or not ref['source_revision'])):
            errors.append('confirmation requires an exact resolved evidence closure')
            return None
        return r

    claim = target(assessment['payload']['claim'])
    if claim and claim['kind'] != 'claim':
        errors.append('assessment must target a claim')
    if assessment.get('schema_version') == 2:
        summary = assessment['payload']['evidence_summary']
        if summary in {'mixed', 'inconclusive', 'unmeasured'}:
            errors.append('mixed or unmeasured evidence cannot yield primary confirmation')
        if (summary, assessment['payload']['verdict']) not in {('supports', 'supported'), ('refutes', 'refuted')}:
            errors.append('confirmation verdict must agree with evidence summary')

    def visit(ref, *, evidence=False):
        r = target(ref)
        if r is None:
            return
        key = (ref['id'], ref['revision_id'], ref['source_revision'])
        if key in visited:
            return
        visited.add(key)
        p = r['payload']
        if evidence and r['provenance']['historical'] and r['kind'] in {'experiment', 'assessment'}:
            errors.append('historical evidence cannot establish native primary confirmation')
        if evidence and claim and claim['payload'].get('domain') == 'nature' and r['scope'] in {'derivation', 'simulation-under-assumptions', 'synthetic-calibration'}:
            errors.append('model or synthetic evidence cannot confirm a claim about nature')
        if r['kind'] == 'artifact' and p['availability'] != 'available':
            errors.append('unavailable artifact cannot support confirmation')
        if r['kind'] == 'artifact' and any('lineage' in missing for missing in r['missingness']):
            errors.append('unresolved or unrecorded dataset lineage blocks confirmation')
        if r['kind'] == 'experiment' and evidence:
            # A run-start receipt is operational lineage, not a result to assess.
            if p['execution_status'] != 'completed' or p['controls'] not in {'passed', 'not-applicable'}:
                errors.append('failed/pending execution or controls cannot support confirmation')
            if not p['protocol'] or not p['result_artifacts']:
                errors.append('confirmation requires protocol and result artifacts')
            if r.get('schema_version') != 2 or not p.get('start') or p.get('deviations'):
                errors.append('confirmation requires native run-start chronology without deviations')
            else:
                start = target(p['start'])
                protocol = target(p['protocol']) if p['protocol'] else None
                if not start or start['kind'] != 'experiment' or start['payload']['execution_status'] != 'running':
                    errors.append('confirmation requires exact running receipt')
                elif not protocol or protocol.get('schema_version') != 2 or protocol['kind'] != 'protocol':
                    errors.append('confirmation requires native registered protocol')
                else:
                    sp, sem = start['payload'], protocol['payload']['semantic']
                    if sem['design']['kind'] != 'deterministic':
                        if p['controls'] != 'passed' or any(p['control_results'].get(name) != 'passed' for name in sem['design']['controls']):
                            errors.append('every frozen control must explicitly pass')
                    elif p['controls'] == 'not-applicable' and p['control_results']:
                        errors.append('inapplicable controls cannot carry measured control results')
                    if sp['protocol'] != p['protocol'] or sp['code'] != sem['code'] or p['code'] != sp['code'] or p['inputs'] != sp['inputs'] or p['inputs'] != sem['inputs'] or p['holdout_digest'] != sem['holdout']['digest'] or sp['holdout_digest'] != p['holdout_digest']:
                        errors.append('run code/data/holdout pins differ from frozen protocol')
                    if instant(protocol['payload']['frozen_at']) > instant(sp['started_at']) or instant(sem['holdout']['evaluation_not_before']) > instant(sp['started_at']) or p['started_at'] != sp['started_at']:
                        errors.append('freeze/start/holdout chronology mismatch')
                    if not any(c['id'] == assessment['payload']['claim']['id'] and c['revision_id'] == assessment['payload']['claim']['revision_id'] for c in sem['claims']):
                        errors.append('protocol did not freeze this claim revision')
        if r['kind'] == 'assessment' and evidence and p.get('verdict') not in {'supported', 'refuted'}:
            errors.append('mixed/limited assessment cannot support primary confirmation')
        for child in _references(r):
            # Superseded records are history, not current evidence. Start is checked above.
            if child in r.get('authorship', {}).get('supersedes', []) or child == p.get('start'):
                target(child)
                continue
            visit(child, evidence=evidence)

    for ref in assessment['payload']['evidence']:
        visit(ref, evidence=True)
    # An artifact-only assertion has no checked control or chronology path.
    if not any(known.get((r['id'], r['revision_id'], r['source_revision']), {}).get('kind') == 'experiment' for r in assessment['payload']['evidence']):
        errors.append('primary confirmation requires an assessed experiment')
    return errors
