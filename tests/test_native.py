from concurrent.futures import ThreadPoolExecutor
from copy import deepcopy
from datetime import datetime, timedelta, timezone
from pathlib import Path
import json
import os
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from orbit_research import Owner, validate
from orbit_research.contract import canonical, reference, revision_digest
from orbit_research.science import protocol_errors

LINK = dict(host='fixture-host', workspace='ws_fixture', task='ORB-fixture', run='jrun-fixture')


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args], text=True).strip()


def initialize(root):
    root.mkdir()
    git(root, 'init', '-q')
    (root/'code.py').write_text('# Fixture only; no experiment is run.\n')
    git(root, 'add', 'code.py')
    git(root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture code')


def commit(root):
    git(root, 'add', 'research/records')
    git(root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'immutable fixture records')
    return git(root, 'rev-parse', 'HEAD')


def request(ident, payload, *, scope='synthetic-calibration', heads=None, key=None):
    return dict(request_id=key or ident, id=ident, payload=payload, scope=scope,
                orbit_links=[LINK], expected_heads=heads or [], reason='Synthetic acceptance fixture.')


def semantic(claim_ref, data_ref, code, *, deterministic=False, boundary=None):
    design = dict(kind='synthetic', samples=dict(total=100, groups=[dict(name='null', count=50), dict(name='signal', count=50)]),
                  baseline='null response', controls=['negative', 'estimator'],
                  decision=dict(metric='detection rate', operator='>=', threshold=.9, attainable_min=0, attainable_max=1),
                  resource_budget=dict(planned=100, limit=200, unit='evaluations'))
    if deterministic:
        design = dict(kind='deterministic', convergence=dict(tolerance=1e-8, max_steps=1000, criterion='residual norm'),
                      decision='residual <= tolerance', resource_budget=dict(planned=1000, limit=1000, unit='steps'))
    return dict(question='Does the fixture satisfy this exact model property?', assumptions='Synthetic apparatus only.',
                analysis='Fixed declared estimator.', exclusions='None.', stopping_rule='Fixed budget.',
                claims=[claim_ref], code=code, inputs=[data_ref], design=design,
                holdout=dict(digest='sha256:'+'d'*64, information_cutoff='2026-01-01T00:00:00+00:00',
                             evaluation_not_before=boundary or (datetime.now(timezone.utc)+timedelta(seconds=1)).isoformat(),
                             policy='Commit seed plan before generating fresh fixture evaluations.'))


class NativeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)/'owner'
        initialize(self.root)
        self.owner = Owner(self.root, 'fixture')
        self.code = dict(repository='fixture', git_revision=git(self.root, 'rev-parse', 'HEAD'))

    def pinned(self, record):
        pin = commit(self.root)
        return reference(self.owner.pin(record['id'], record['revision_id'], pin), 'resolved')

    def setup_protocol(self, *, deterministic=False):
        c = self.owner.apply('claim', request('C1', dict(role='claim', statement='A fixture model property.', domain='model')))
        c_ref = self.pinned(c)
        data = self.owner.apply('artifact', request('D1', dict(role='dataset', availability='available', snapshot_digest='sha256:'+'d'*64,
                                                             locator='fixture:seed-plan', media_type='application/json')))
        data_ref = self.pinned(data)
        sem = semantic(c_ref, data_ref, self.code, deterministic=deterministic)
        p = self.owner.apply('preregister', request('P1', dict(semantic=sem)))
        p_ref = self.pinned(p)
        return c_ref, data_ref, p, p_ref

    def run_payload(self, p_ref, data_ref, *, status='running', start=None, controls='not-run', results=None):
        return dict(execution_status=status, controls=controls, protocol=p_ref, result_artifacts=results or [], inputs=[data_ref],
                    code=self.code, environment={'python':'fixture'}, invocation=['python', 'code.py'], deviations=[],
                    start=start, holdout_digest='sha256:'+'d'*64, control_results={'negative':controls,'estimator':controls})

    def test_idempotency_stale_base_forks_retirement_and_old_bytes(self):
        req = request('C1', dict(role='claim', statement='Original.', domain='model'))
        first = self.owner.apply('claim', req)
        before = {p:p.read_bytes() for p in self.owner.directory.glob('*.json')}
        self.assertEqual(first, self.owner.apply('claim', req))
        changed = deepcopy(req); changed['payload']['statement']='Changed.'
        with self.assertRaisesRegex(ValueError, 'idempotency'):
            self.owner.apply('claim', changed)
        changed['request_id']='revision2'
        with self.assertRaisesRegex(ValueError, 'stale base'):
            self.owner.apply('claim', changed)
        changed['expected_heads']=[first['revision_id']]
        second = self.owner.apply('claim', changed)
        fork = deepcopy(changed); fork.update(request_id='fork', expected_heads=[second['revision_id']], supersedes=[])
        fork['payload']['statement']='Disputed alternative.'
        third = self.owner.apply('claim', fork)
        self.assertEqual(sorted([second['revision_id'], third['revision_id']]), self.owner.heads(first['id']))
        retire = request('C1', second['payload'], heads=self.owner.heads(first['id']), key='retire')
        retire.update(kind='claim', supersedes=[second['revision_id']])
        retired = self.owner.apply('retire', retire)
        self.assertEqual('retired', retired['activity'])
        self.assertIn(third['revision_id'], self.owner.heads(first['id']))
        self.assertEqual(before, {p:p.read_bytes() for p in before})

    def test_concurrent_stale_writers_only_one_append(self):
        reqs = [request('C1', dict(role='claim', statement=str(n), domain='model'), key=str(n)) for n in range(2)]
        def append(req):
            try:
                return self.owner.apply('claim', req)
            except ValueError:
                return None
        with ThreadPoolExecutor(2) as pool:
            values = list(pool.map(append, reqs))
        self.assertEqual(1, sum(v is not None for v in values))
        self.assertEqual(1, len(self.owner.records()))

    def test_write_failure_before_and_after_publication_is_retryable(self):
        req=request('C1',dict(role='claim', statement='Original.', domain='model'))
        with patch('orbit_research.native.os.link', side_effect=OSError('disk full')):
            with self.assertRaisesRegex(OSError, 'disk full'):
                self.owner.apply('claim', req)
        self.assertEqual([], self.owner.records())
        self.assertEqual([], list(self.owner.directory.iterdir()))
        real_fsync = os.fsync
        calls = []
        def fsync(fd):
            calls.append(fd)
            if len(calls) == 2:
                raise OSError('directory fsync failed after publication')
            real_fsync(fd)
        with patch('orbit_research.native.os.fsync', side_effect=fsync):
            with self.assertRaises(OSError):
                self.owner.apply('claim', req)
        self.assertEqual(1, len(self.owner.records()))
        self.assertEqual(self.owner.records()[0], self.owner.apply('claim', req))

    def test_tampering_and_missing_chain_fail_closed(self):
        req=request('C1',dict(role='claim', statement='Original.', domain='model'))
        self.owner.apply('claim', req)
        path=next(self.owner.directory.glob('*.json'))
        r=json.loads(path.read_text()); r['presentation']['note']='tampered'
        path.write_text(json.dumps(r))
        with self.assertRaisesRegex(ValueError,'filename/content'):
            self.owner.records()

    def test_deleted_committed_tail_is_not_silently_replaced(self):
        self.owner.apply('claim',request('C1',dict(role='claim',statement='Original.',domain='model')))
        commit(self.root)
        next(self.owner.directory.glob('*.json')).unlink()
        with self.assertRaisesRegex(ValueError,'committed append was removed'):
            self.owner.apply('claim',request('C2',dict(role='claim',statement='Replacement.',domain='model')))

    def test_exact_cross_owner_source_routing(self):
        other_root=Path(self.tmp.name)/'sibling'
        initialize(other_root)
        other=Owner(other_root,'sibling')
        program=other.apply('program',request('R1',dict(role='program',title='Sibling program',question='Exact sibling question?')))
        pin=commit(other_root)
        ref=reference(other.pin(program['id'],program['revision_id'],pin),'resolved')
        req=request('C1',dict(role='claim',statement='Cross-owner claim.',domain='model'))
        req['references']=[ref]
        with self.assertRaisesRegex(ValueError,'unrouted'):
            self.owner.apply('claim',req)
        routed=Owner(self.root,'fixture',sources={'sibling':other_root})
        claim=routed.apply('claim',req)
        bundle=routed.export(commit(self.root))
        self.assertEqual([],validate(bundle))
        self.assertTrue(any(m['repositories'][0]['id']=='sibling' for m in bundle['manifests']))
        wrong=deepcopy(req); wrong.update(request_id='wrong-pin',id='C2'); wrong['references'][0]['source_revision']='f'*40
        with self.assertRaises(ValueError):
            routed.apply('claim',wrong)

    def test_path_and_owner_guards(self):
        outside=Path(self.tmp.name)/'outside'; outside.mkdir()
        (self.root/'research').symlink_to(outside)
        with self.assertRaisesRegex(ValueError,'symlinks'):
            Owner(self.root,'fixture')
        with self.assertRaises(ValueError):
            Owner(self.root,'fixture',records='../outside')
        (self.root/'nested').mkdir()
        with self.assertRaises(ValueError):
            Owner(self.root/'nested','fixture')

    def test_freeze_arithmetic_and_no_backdating(self):
        c_ref, data_ref, p, p_ref = self.setup_protocol()
        self.assertEqual('registered', p['payload']['freeze'])
        for change in ['count', 'threshold', 'binomial', 'budget']:
            sem=deepcopy(p['payload']['semantic'])
            if change=='count': sem['design']['samples']['total']=47
            if change=='threshold': sem['design']['decision']['threshold']=1.1
            if change=='binomial': sem['design']['decision'].update(threshold=.99, binomial_lower_bound={'n':44,'confidence':.95})
            if change=='budget': sem['design']['resource_budget']['limit']=99
            self.assertTrue(protocol_errors(sem), change)
        req=request('P2', dict(semantic=p['payload']['semantic'], frozen_at='2020-01-01'))
        with self.assertRaisesRegex(ValueError,'caller freeze'):
            self.owner.apply('preregister',req)
        old=deepcopy(p['payload']['semantic']); old['holdout']['evaluation_not_before']='2020-01-01T00:00:00Z'
        with self.assertRaises(ValueError):
            self.owner.apply('preregister',request('P2',dict(semantic=old)))
        self.assertEqual([], protocol_errors(semantic(c_ref,data_ref,self.code,deterministic=True)))
        start_req=request('E1',self.run_payload(p_ref,data_ref))
        with patch('orbit_research.native.now', return_value=p['payload']['frozen_at']):
            with self.assertRaisesRegex(ValueError,'boundary'):
                self.owner.apply('begin-run',start_req)
        before={path:path.read_bytes() for path in self.owner.directory.glob('*.json')}
        corrected=deepcopy(p['payload']['semantic'])
        corrected['analysis']='A new, explicitly corrected estimator.'
        corrected['holdout']['evaluation_not_before']=(datetime.now(timezone.utc)+timedelta(seconds=1)).isoformat()
        revised=self.owner.apply('preregister',request('P1',dict(semantic=corrected),heads=[p['revision_id']],key='protocol-correction'))
        self.assertNotEqual(p['revision_id'],revised['revision_id'])
        self.assertEqual(p['revision_id'],revised['authorship']['supersedes'][0]['revision_id'])
        self.assertEqual(before,{path:path.read_bytes() for path in before})

    def test_complete_failed_and_successful_evidence_closure_and_export(self):
        c_ref, data_ref, p, p_ref=self.setup_protocol()
        boundary=p['payload']['semantic']['holdout']['evaluation_not_before']
        with patch('orbit_research.native.now', return_value=boundary):
            start=self.owner.apply('begin-run',request('E1',self.run_payload(p_ref,data_ref)))
        s_ref=self.pinned(start)
        # Keep measured registration monotonically after the synthetic evaluation boundary.
        later=(datetime.fromisoformat(boundary)+timedelta(seconds=1)).isoformat()
        with patch('orbit_research.native.now', return_value=later):
            result=self.owner.apply('artifact',request('O1',dict(role='result',availability='available',snapshot_digest='sha256:'+'e'*64,locator='fixture:result',media_type='application/json')))
        result_ref=self.pinned(result)
        with patch('orbit_research.native.now', return_value=later):
            run=self.owner.apply('record-run',request('E1',self.run_payload(p_ref,data_ref,status='completed',start=s_ref,controls='passed',results=[result_ref]),heads=[start['revision_id']],key='finish'))
        r_ref=self.pinned(run)
        payload=dict(claim=c_ref, verdict='supported', inference='confirmatory-primary', controls='passed',basis='scientific-evidence',
                     rationale='Synthetic criterion met; model scope only.',evidence=[r_ref],legacy_verdict=None,evidence_summary='supports')
        with patch('orbit_research.native.now', return_value=later):
            a=self.owner.apply('assess',request('A1',payload))
        export=self.owner.export(commit(self.root))
        before=deepcopy(export)
        self.assertEqual([],validate(export))
        self.assertEqual(before,export, 'validation must not mutate manifests or records')
        self.assertTrue(all(ref['status']=='resolved' for m in export['manifests'] for ref in m['references']))
        trace=self.owner.trace(c_ref['id'],c_ref['revision_id'])
        self.assertIn(a['id'],[r['id'] for r in trace['records']])
        # These cases must fail even if assessment asserts passing controls.
        all_records=export['records']
        for state in ['failed','not-run','unknown']:
            bad=deepcopy(run); bad['payload']['controls']=state; bad['revision_id']=revision_digest(bad)
            bad['provenance'].update(working_tree=False,git_revision=r_ref['source_revision'])
            assessment=deepcopy(a); assessment['payload']['evidence']=[reference(bad,'resolved')]; assessment['revision_id']=revision_digest(assessment)
            targets=[r for r in all_records if r['id'] != a['id']]+[bad]
            self.assertTrue(validate(assessment,targets=targets),state)
        mixed=deepcopy(a); mixed['payload']['evidence_summary']='mixed'; mixed['revision_id']=revision_digest(mixed)
        self.assertTrue(validate(mixed,targets=[r for r in all_records if r['id'] != a['id']]))
        # Wrong source pins, missing control results and nature scope stay ineligible.
        for field in ['pin','control','nature']:
            assessment=deepcopy(a)
            targets=[r for r in all_records if r['id'] != a['id']]
            if field=='pin':
                assessment['payload']['evidence'][0]['source_revision']='f'*40
            elif field=='control':
                bad=deepcopy(run); bad['payload']['control_results']['negative']='not-run'
                bad['revision_id']=revision_digest(bad); bad['provenance'].update(working_tree=False,git_revision=r_ref['source_revision'])
                targets.append(bad); assessment['payload']['evidence']=[reference(bad,'resolved')]
            else:
                nature=deepcopy(next(r for r in all_records if r['id']==c_ref['id']))
                nature['payload']['domain']='nature'; nature['revision_id']=revision_digest(nature)
                targets.append(nature); assessment['payload']['claim']=reference(nature,'resolved')
                assessment['payload']['inference']='exploratory'
            assessment['revision_id']=revision_digest(assessment)
            self.assertTrue(validate(assessment,targets=targets),field)
        with patch('orbit_research.native.now', return_value=later):
            failed=self.owner.apply('record-run',request('E-failed',self.run_payload(p_ref,data_ref,status='failed',controls='failed')))
            f_ref=self.pinned(failed)
            narrow=deepcopy(payload); narrow.update(verdict='inconclusive',inference='exploratory',controls='failed',evidence=[f_ref],evidence_summary='inconclusive')
            self.owner.apply('assess',request('A-failed',narrow))
            correction_payload=self.run_payload(p_ref,data_ref,status='cancelled',controls='not-run')
            corrected=self.owner.apply('record-run',request('E-failed',correction_payload,heads=[failed['revision_id']],key='failure-correction'))
            self.assertEqual(failed['revision_id'],corrected['authorship']['supersedes'][0]['revision_id'])
            self.assertEqual('failed',failed['payload']['execution_status'])
        self.assertEqual('supported',a['payload']['verdict'])

    def test_uncommitted_freeze_and_export_refused(self):
        c=self.owner.apply('claim',request('C1',dict(role='claim',statement='X',domain='model')))
        with self.assertRaisesRegex(ValueError,'no committed source'):
            self.owner.export(self.code['git_revision'])


if __name__=='__main__':
    unittest.main()
