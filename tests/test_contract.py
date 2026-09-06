from copy import deepcopy
import unittest

from orbit_research.contract import (make_record, protocol_digest, reference, reconcile,
                                     revision_digest, validate)

PIN = 'a' * 40
PROV = dict(repository='demo', git_revision=PIN, blob_oid='b'*40, sha256='sha256:'+'c'*64,
            path='records.json', selector='$', historical=False, working_tree=False)


def claim(**kwargs):
    return make_record('demo', 'claim', 'C1', dict(role='claim', statement='A model property.', domain='model'), PROV, scope='derivation', **kwargs)


def protocol(semantic=None, historical=False):
    semantic = semantic or dict(hypothesis='C1', comparator='zero coupling', decision='error < 0.01', controls=['null'], analysis='fixed estimator')
    return make_record('demo', 'protocol', 'P1', dict(semantic=semantic, semantic_digest=protocol_digest(semantic),
                       freeze='historical-unverified', frozen_at=None, freeze_evidence=None),
                       {**PROV, 'historical':historical}, scope='derivation')


def assessment(verdict='inconclusive', controls='failed', inference='exploratory', historical=False, legacy_verdict=None):
    return make_record('demo', 'assessment', 'A1', dict(claim=reference(claim()), verdict=verdict, controls=controls,
                       inference=inference, basis='legacy-report' if historical else 'scientific-evidence',
                       rationale='Control failure prevents primary inference.', evidence=[], legacy_verdict=legacy_verdict),
                       {**PROV, 'historical':historical}, scope='synthetic-calibration')


def revise(r):
    r['revision_id'] = revision_digest(r)
    return r


class ContractTests(unittest.TestCase):
    def test_protocol_presentation_does_not_change_freeze(self):
        before = protocol()
        after = deepcopy(before)
        after['presentation'] = {'explanation':'Later explanatory prose.'}
        after['provenance']['git_revision'] = 'd'*40
        self.assertEqual(before['revision_id'], revision_digest(after))
        self.assertEqual([], validate(after))
        after['payload']['semantic']['decision'] = 'error < 0.1'
        self.assertNotEqual(before['revision_id'], revision_digest(after))
        self.assertTrue(validate(after))
        new = protocol(after['payload']['semantic'])
        self.assertEqual(before['id'], new['id'])
        self.assertNotEqual(before['revision_id'], new['revision_id'])
        self.assertEqual([], validate(new))

    def test_historical_freeze_cannot_be_fabricated(self):
        p = protocol(historical=True)
        self.assertEqual([], validate(p))
        p['payload'].update(freeze='prospective', frozen_at='2020-01-01', freeze_evidence=reference(claim()))
        self.assertIn('historical import cannot fabricate prospective preregistration', validate(p))

    def test_activity_execution_and_verdict_independent(self):
        run = make_record('demo','experiment','E1',dict(execution_status='completed', controls='failed',protocol=None,result_artifacts=[]),PROV,activity='retired')
        result = assessment()
        self.assertEqual([], validate(run))
        self.assertEqual([], validate(result))
        result['activity'] = 'retired'
        self.assertEqual([], validate(revise(result)))
        self.assertEqual('inconclusive', result['payload']['verdict'])
        result['payload'].update(inference='confirmatory-primary',verdict='supported')
        self.assertIn('confirmatory primary inference requires passing controls', validate(revise(result)))

    def test_task_completion_never_supports_claim(self):
        result = assessment(verdict='supported', controls='passed')
        result['payload']['basis'] = 'execution-only'
        self.assertTrue(any('execution success' in e for e in validate(revise(result))))

    def test_historical_verdicts_preserved(self):
        for raw, canonical in [('mixed','inconclusive'),('untested','untested'),('conditional','conditional'),('supported','supported')]:
            result = assessment(canonical, inference='historical', historical=True, legacy_verdict=raw)
            self.assertEqual([], validate(result))
            result['payload']['verdict'] = 'supported' if canonical != 'supported' else 'refuted'
            self.assertIn('historical verdict was strengthened or changed', validate(revise(result)))

    def test_model_property_remains_model_property(self):
        c = claim()
        self.assertEqual('model', c['payload']['domain'])
        self.assertEqual('derivation', c['scope'])
        self.assertNotIn('verdict', c['payload'])
        c['payload']['verdict'] = 'supported'
        self.assertTrue(validate(c))

    def test_stable_identity_not_path(self):
        c = claim()
        moved = deepcopy(c)
        moved['provenance']['path'] = 'elsewhere.json'
        self.assertEqual(c['revision_id'], revision_digest(moved))
        moved['provenance']['repository'] = 'other'
        self.assertTrue(validate(moved))

    def test_pending_reconciliation_and_wrong_pins(self):
        c = claim()
        ref = reference(c)
        manifest = dict(schema_version=1,kind='manifest',repositories=[dict(id='demo',git_revision=PIN)], references=[ref])
        self.assertEqual([], validate(manifest))
        self.assertEqual('pending', reconcile(manifest, [])['references'][0]['status'])
        self.assertEqual('resolved', reconcile(manifest, [c])['references'][0]['status'])
        self.assertEqual('pending', manifest['references'][0]['status'])
        for field, value in [('revision_id','sha256:'+'f'*64),('source_revision','f'*40),('repository','elsewhere')]:
            bad = deepcopy(manifest)
            bad['references'][0][field] = value
            self.assertEqual('pending',reconcile(bad,[c])['references'][0]['status'])
        self.assertEqual('pending',reconcile(manifest,[c,c])['references'][0]['status'])

    def test_pending_evidence_cannot_confirm(self):
        a = assessment('supported', 'passed', 'confirmatory-primary')
        a['payload']['evidence'] = [reference(claim())]
        self.assertIn('pending references cannot be current confirmatory evidence', validate(revise(a)))

    def test_malformed_records_fail_structurally(self):
        for value in [[], {}, {'schema_version':2}, {'schema_version':[]}, {'schema_version':True}, {'kind':'claim'}]:
            self.assertTrue(validate(value))
        c = claim()
        c['provenance']['git_revision'] = 'main'
        self.assertTrue(validate(c))


if __name__ == '__main__':
    unittest.main()

class ReferenceBoundaryTests(unittest.TestCase):
    def test_manifest_can_validate_with_exact_targets(self):
        c=claim()
        manifest=dict(schema_version=1,kind='manifest',repositories=[dict(id='demo',git_revision=PIN)],references=[reference(c)])
        resolved=reconcile(manifest,[c])
        self.assertEqual([],validate(resolved,targets=[c]))
        self.assertTrue(validate(resolved))
        c['provenance']['working_tree']=True
        self.assertEqual('pending',reconcile(manifest,[c])['references'][0]['status'])

    def test_failed_control_target_overrides_assessment_assertion(self):
        c=claim()
        run=make_record('demo','experiment','E1',dict(execution_status='completed',controls='failed',protocol=None,result_artifacts=[]),PROV,scope='synthetic-calibration')
        a=assessment('supported','passed','confirmatory-primary')
        a['payload']['claim']=reference(c,'resolved')
        a['payload']['evidence']=[reference(run,'resolved')]
        self.assertIn('failed-control experiment cannot support primary confirmation',validate(revise(a),targets=[c,run]))

    def test_model_evidence_cannot_confirm_nature(self):
        c=claim()
        c['payload']['domain']='nature'
        revise(c)
        run=make_record('demo','experiment','E1',dict(execution_status='completed',controls='passed',protocol=None,result_artifacts=[]),PROV,scope='simulation-under-assumptions')
        a=assessment('supported','passed','confirmatory-primary')
        a['payload']['claim']=reference(c,'resolved')
        a['payload']['evidence']=[reference(run,'resolved')]
        self.assertIn('model or synthetic evidence cannot confirm a claim about nature',validate(revise(a),targets=[c,run]))

    def test_alias_encoding_is_injective(self):
        payload=claim()['payload']
        ids=[make_record('demo','claim',s,payload,PROV)['id'] for s in ['a:b','a%3Ab','a/b','a b','α']]
        self.assertEqual(len(ids),len(set(ids)))
        self.assertTrue(all('/' not in ident and ' ' not in ident for ident in ids))
