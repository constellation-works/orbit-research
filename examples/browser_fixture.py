"""Four disposable owner checkouts for browser/reconciliation acceptance; no science runs."""
import base64
import json
from pathlib import Path
import subprocess
import sys

from orbit_research import import_source, make_record
from orbit_research.contract import canonical, digest_bytes, reference, revision_digest


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args], text=True).strip()


def create(destination):
    root = Path(destination)
    root.mkdir(parents=True, exist_ok=False)
    records, pins, roots = [], {}, {}
    for repo in ('principia', 'parallax', 'orrery', 'astrolabe'):
        owner = root / repo
        owner.mkdir(); roots[repo] = owner
        git(owner, 'init', '-q')
        (owner / 'source.txt').write_text('Synthetic acceptance source; no empirical findings.\n')
        if repo == 'orrery':
            (owner / 'simulation.html').write_text('<!doctype html><title>Fixture simulation</title><p>Explicit navigation fixture. No simulation run.</p>')
            (owner / 'figure.png').write_bytes(base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aEuoAAAAASUVORK5CYII='))
        if repo == 'parallax':
            docs = owner / 'docs/research/R01-fixture'; docs.mkdir(parents=True)
            (docs / 'H08.md').write_text('---\nresearch_id: R01\nhypothesis_id: H08\nstatus: archived\noutcome: revised\n---\n\n# H08 Night flattening fixture\n\n## Claim\n\nDoes night-window drift identify inventory flattening?\n\n## Limitations\n\nClock seasonality cannot identify who sold or why.\n')
            (docs / 'E01.md').write_text('---\nresearch_id: R01\nexperiment_id: E01\nhypothesis_id: H08\nstatus: completed\noutcome: revised\npreregistered: false\n---\n\n# E01 Exploratory hourly drift fixture\n\n## Methodology\n\nWindows were chosen after viewing hourly tables.\n\n## Outcome\n\nExploratory only; cannot advance H08. Requires unseen data and a new frozen protocol.\n')
        git(owner, 'add', '.')
        git(owner, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'synthetic acceptance source')
        pins[repo] = git(owner, 'rev-parse', 'HEAD')

    def record(repo, kind, ident, payload, *, missing=(), refs=(), title=None, legacy=None):
        provenance = dict(repository=repo, git_revision=pins[repo], blob_oid=git(roots[repo], 'rev-parse', 'HEAD:source.txt'),
                          sha256=digest_bytes((roots[repo] / 'source.txt').read_bytes()), path='source.txt',
                          selector='$', historical=True, working_tree=False)
        r = make_record(repo, kind, ident, payload, provenance, activity='active', scope='synthetic-calibration',
                        limitations=['Synthetic acceptance fixture; no claim about nature.'], missingness=missing, legacy=legacy)
        r['references'] = list(refs)
        r['presentation']['title'] = title or ident
        r['revision_id'] = revision_digest(r)
        records.append(r)
        return r

    program = record('principia', 'program', 'wide-binary', dict(role='program',title='Wide-binary control program'))
    claim = record('principia', 'claim', 'wide-binary-power', dict(role='claim',domain='model',statement='The injected-effect control detects the prespecified signal with at least 80% power.'),
                   refs=[reference(program)], title='Wide-binary power remains unresolved')
    dataset = record('astrolabe', 'artifact', 'unpersisted-input',dict(role='dataset',availability='missing',snapshot_digest=None,locator='data/never-persisted.parquet',media_type='application/vnd.apache.parquet'),missing=['input lineage was never persisted'])
    sem = dict(question='Can the control detect a known injected effect?',assumptions='Synthetic population only.', code={'repository':'orrery','git_revision':pins['orrery']},inputs=[reference(dataset)])
    protocol = record('principia', 'protocol', 'frozen-control',dict(semantic=sem,semantic_digest=digest_bytes(canonical(sem)),freeze='historical-unverified',frozen_at=None,freeze_evidence=None),refs=[reference(claim),reference(dataset)])
    result = record('orrery','artifact','missing-result',dict(role='result',availability='missing',snapshot_digest=None,locator='assets/results.json',media_type='application/json'))
    run = record('orrery','experiment','control-run',dict(execution_status='completed',controls='failed',protocol=reference(protocol),result_artifacts=[reference(result)]),refs=[reference(dataset)],title='Completed wide-binary control run')
    assessment = record('principia','assessment','power-assessment',dict(claim=reference(claim),verdict='inconclusive',inference='historical',controls='failed',basis='legacy-report',rationale='Fixture reproduces the recorded question: 33.3% detection falls below the 80% rule. A null remains uninformative.',evidence=[reference(run)],legacy_verdict='mixed'),legacy={'status':'mixed'},title='Power control failed; inference remains open')
    record('orrery','program','simulation',dict(role='program',title='Wide-binary simulation catalog'),refs=[reference(program)])
    report = import_source(roots['parallax'], 'parallax', 'parallax')
    report_path = root / 'parallax-report.json'; report_path.write_text(json.dumps(report))
    manifests = []
    for repo in roots:
        subset = [r for r in records if r['provenance']['repository'] == repo]
        if subset:
            manifests.append(dict(schema_version=1,kind='manifest',repositories=[dict(id=repo,git_revision=pins[repo])],references=[reference(r) for r in subset]))
    docs = root / 'owner-records'; docs.mkdir()
    for i,r in enumerate(records): (docs / f'{i:02}.json').write_text(json.dumps(r))
    for i,m in enumerate(manifests): (docs / f'manifest-{i}.json').write_text(json.dumps(m))
    media = []
    for path, role in [('figure.png','illustration'),('simulation.html','simulation')]:
        media.append(dict(label='Synthetic apparatus '+role,role=role,record=reference(run),repository='orrery',path=path,source_revision=pins['orrery'],sha256=digest_bytes((roots['orrery']/path).read_bytes())))
    config = dict(schema_version=1,checkouts={r:str(p) for r,p in roots.items()},documents=[str(docs),str(report_path)],media=media,checkout_urls={'orrery':'/orrery/'})
    config_path = root / 'index-config.json'; config_path.write_text(json.dumps(config,indent=2)+'\n')
    return config_path


if __name__ == '__main__':
    print(create(Path(sys.argv[1]).resolve()))
