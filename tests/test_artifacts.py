from copy import deepcopy
import json
from pathlib import Path
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from unittest.mock import patch

from orbit_research import Owner, make_record, reconcile, validate, verify_artifact
from orbit_research.contract import canonical, digest_bytes, reference, revision_digest
from test_native import initialize, commit, request, semantic, git


def fixture(root, *, unresolved=False):
    root.mkdir()
    # Portable stand-in format. The descriptor has the exact delivered Astrolabe
    # field structure; actual Parquet schema parsing remains an owner responsibility.
    data=b'column:x\n1\n2\n'
    meta=dict(name='tiny',kind='catalog',columns=['x'],n_rows=2,units={'x':'m'},lineage=[{'dataset':None}] if unresolved else [])
    sidecar=canonical(meta)
    descriptor=dict(format='astrolabe-dataset-snapshot-v1',dataset={'kind':'catalog','name':'tiny'},
                    parquet_sha256=digest_bytes(data),sidecar_sha256=digest_bytes(sidecar),
                    schema=[{'name':'x','type':'int64','nullable':False}],parquet_metadata={},units={'x':'m'},parent_pins=[])
    record=make_record('astrolabe','artifact','dataset:catalog:tiny',
                       dict(role='dataset',availability='available',snapshot_digest=digest_bytes(canonical(descriptor)),locator='dataset.parquet',media_type='application/vnd.apache.parquet'),
                       dict(repository='astrolabe',git_revision=None,blob_oid=None,sha256=digest_bytes(sidecar),path='metadata.json',selector='$',historical=False,working_tree=True),
                       legacy={'dataset':meta,'snapshot':descriptor,'lineage_resolution':[{'status':'pending','legacy':{'dataset':None}}] if unresolved else []},
                       activity='active',scope='observation',missingness=['non-git-dataset-bytes']+(['historical-lineage-unresolved'] if unresolved else []))
    (root/'dataset.parquet').write_bytes(data); (root/'metadata.json').write_bytes(sidecar)
    (root/'record.json').write_bytes(canonical(record))
    return record,descriptor


def schema_check(root, record, descriptor):
    # Owner parser validates actual bytes, row/schema/units and retained sidecar.
    data=(root/'dataset.parquet').read_text().splitlines()
    meta=json.loads((root/'metadata.json').read_text())
    if data[0]!='column:x' or len(data)-1!=meta['n_rows'] or meta['columns']!=['x'] or descriptor['schema']!=[{'name':'x','type':'int64','nullable':False}] or meta['units']!=descriptor['units'] or meta!=record['legacy']['dataset']:
        raise ValueError('owner schema/sidecar mismatch')


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name)/'snapshot'
        self.record,self.descriptor=fixture(self.root)

    def proof(self, **overrides):
        args=dict(root=self.root,descriptor=self.descriptor,byte_fields={'parquet_sha256':'dataset.parquet','sidecar_sha256':'metadata.json'},semantic_check=schema_check)
        args.update(overrides)
        return verify_artifact(self.record,**args)

    def test_non_git_bytes_resolve_only_with_exact_recheckable_owner_proof(self):
        proof=self.proof()
        ref=reference(self.record)
        resolver=lambda pin: proof if pin==ref or pin=={**ref,'status':'resolved'} else None
        manifest=dict(schema_version=1,kind='manifest',repositories=[dict(id='astrolabe',git_revision=None)],references=[ref])
        original=deepcopy(manifest)
        self.assertEqual('pending',reconcile(manifest,[self.record])['references'][0]['status'])
        resolved=reconcile(manifest,[self.record],artifact_resolver=resolver)
        self.assertEqual('resolved',resolved['references'][0]['status'])
        self.assertIsNone(resolved['references'][0]['source_revision'])
        self.assertEqual([],validate(resolved,targets=[self.record],artifact_resolver=resolver))
        self.assertTrue(validate(resolved,targets=[self.record]))
        self.assertEqual(original,manifest)
        # Proof is not a permanent pass: tampering after initial verification fails.
        (self.root/'dataset.parquet').write_bytes(b'tampered')
        self.assertTrue(validate(resolved,targets=[self.record],artifact_resolver=resolver))
        self.assertEqual('pending',reconcile(manifest,[self.record],artifact_resolver=resolver)['references'][0]['status'])

    def test_complete_members_schema_parent_and_sidecar_pins_are_required(self):
        with self.assertRaisesRegex(ValueError,'every descriptor byte'):
            self.proof(byte_fields={'parquet_sha256':'dataset.parquet'})
        descriptor=deepcopy(self.descriptor); descriptor['schema'][0]['type']='float64'
        with self.assertRaisesRegex(ValueError,'descriptor digest'):
            self.proof(descriptor=descriptor)
        descriptor=deepcopy(self.descriptor); descriptor['parent_pins']=[reference(self.record)]
        with self.assertRaises(ValueError): self.proof(descriptor=descriptor)
        with self.assertRaisesRegex(ValueError,'owner schema'):
            self.proof(semantic_check=lambda *args: (_ for _ in ()).throw(ValueError('owner schema mismatch')))
        (self.root/'metadata.json').unlink()
        with self.assertRaisesRegex(ValueError,'missing artifact'):
            self.proof()

    def test_mutation_during_owner_schema_check_and_wrong_record_are_rejected(self):
        def changing(root,record,descriptor):
            (root/'metadata.json').write_bytes(b'changed during parse')
        with self.assertRaisesRegex(ValueError,'changed during'):
            self.proof(semantic_check=changing)
        (self.root/'metadata.json').write_bytes(canonical(self.record['legacy']['dataset']))
        bad=deepcopy(self.record); bad['presentation']['changed']=True
        (self.root/'record.json').write_bytes(canonical(bad))
        with self.assertRaisesRegex(ValueError,'record was changed'):
            self.proof()

    def test_missing_historical_bytes_cannot_be_promoted(self):
        self.record['payload'].update(availability='missing',snapshot_digest=None)
        self.record['revision_id']=revision_digest(self.record)
        (self.root/'record.json').write_bytes(canonical(self.record))
        with self.assertRaisesRegex(ValueError,'only available'):
            self.proof()

    def test_verified_bytes_do_not_resolve_missing_parent_lineage(self):
        root=Path(self.tmp.name)/'unresolved'
        record,descriptor=fixture(root,unresolved=True)
        proof=verify_artifact(record,root=root,descriptor=descriptor,
            byte_fields={'parquet_sha256':'dataset.parquet','sidecar_sha256':'metadata.json'},semantic_check=schema_check)
        resolver=lambda ref: proof if ref['id']==record['id'] else None
        provenance={**self.record['provenance'],'repository':'fixture','git_revision':'a'*40,'working_tree':False}
        claim=make_record('fixture','claim','C1',dict(role='claim',statement='An empirical claim.',domain='empirical'),provenance,scope='observation')
        assessment=make_record('fixture','assessment','A1',dict(claim=reference(claim,'resolved'),verdict='supported',inference='confirmatory-primary',controls='passed',basis='scientific-evidence',rationale='Fixture.',evidence=[reference(record,'resolved')],legacy_verdict=None),provenance,scope='observation')
        errors=validate(assessment,targets=[claim,record],artifact_resolver=resolver)
        self.assertIn('unresolved or unrecorded dataset lineage blocks confirmation',errors)

    def test_native_export_uses_external_resolver_without_git_data_or_false_lineage(self):
        owner_root=Path(self.tmp.name)/'owner'; initialize(owner_root)
        proof=self.proof(); ref=reference(self.record,'resolved')
        resolver=lambda pin: proof if pin['id']==ref['id'] and pin['revision_id']==ref['revision_id'] and pin['source_revision'] is None else None
        owner=Owner(owner_root,'fixture',artifact_resolver=resolver)
        req=request('C1',dict(role='claim',statement='Uses an exact retained dataset.',domain='empirical'),scope='observation')
        req['references']=[ref]
        owner.apply('claim',req)
        exported=owner.export(commit(owner_root))
        self.assertEqual([],validate(exported,artifact_resolver=resolver))
        self.assertTrue(any(r['provenance']['git_revision'] is None for r in exported['records']))
        self.assertFalse((owner_root/'dataset.parquet').exists())
        # Byte resolution does not infer a parent for a missing legacy lineage entry.
        other_root=Path(self.tmp.name)/'unresolved'
        record,descriptor=fixture(other_root,unresolved=True)
        unresolved_proof=verify_artifact(record,root=other_root,descriptor=descriptor,
            byte_fields={'parquet_sha256':'dataset.parquet','sidecar_sha256':'metadata.json'},semantic_check=schema_check)
        self.assertIn('historical-lineage-unresolved',unresolved_proof.verify()['missingness'])
        self.assertEqual([{'status':'pending','legacy':{'dataset':None}}],unresolved_proof.verify()['legacy']['lineage_resolution'])

    def test_primary_native_workflow_can_use_verified_non_git_input(self):
        root=Path(self.tmp.name)/'native'; initialize(root)
        proof=self.proof(); d_ref=reference(self.record,'resolved')
        resolver=lambda ref: proof if ref['id']==d_ref['id'] and ref['revision_id']==d_ref['revision_id'] and ref['source_revision'] is None else None
        owner=Owner(root,'fixture',artifact_resolver=resolver)
        code=dict(repository='fixture',git_revision=git(root,'rev-parse','HEAD'))
        def publish(r):
            return reference(owner.pin(r['id'],r['revision_id'],commit(root)),'resolved')
        claim=owner.apply('claim',request('C1',dict(role='claim',statement='Model fixture.',domain='model')))
        c_ref=publish(claim)
        sem=semantic(c_ref,d_ref,code)
        sem['holdout']['digest']=self.record['payload']['snapshot_digest']
        protocol=owner.apply('preregister',request('P1',{'semantic':sem}))
        p_ref=publish(protocol)
        run=dict(execution_status='running',controls='not-run',control_results={'negative':'not-run','estimator':'not-run'},
                 protocol=p_ref,inputs=[d_ref],code=code,result_artifacts=[],start=None,holdout_digest=sem['holdout']['digest'],
                 environment={'fixture':True},invocation=['python','code.py'],deviations=[])
        later=(datetime.fromisoformat(sem['holdout']['evaluation_not_before'])+timedelta(seconds=1)).isoformat()
        with patch('orbit_research.native.now',return_value=later):
            start=owner.apply('begin-run',request('E1',run)); s_ref=publish(start)
            result=owner.apply('artifact',request('O1',dict(role='result',availability='available',snapshot_digest=digest_bytes(b'fixture-output'),locator='fixture:result',media_type='text/plain')))
            result_ref=publish(result)
            run.update(execution_status='completed',controls='passed',control_results={'negative':'passed','estimator':'passed'},start=s_ref,result_artifacts=[result_ref])
            completed=owner.apply('record-run',request('E1',run,heads=[start['revision_id']],key='finish'))
            e_ref=publish(completed)
            assessment=owner.apply('assess',request('A1',dict(claim=c_ref,verdict='supported',inference='confirmatory-primary',controls='passed',basis='scientific-evidence',rationale='Fixture only.',evidence=[e_ref],legacy_verdict=None,evidence_summary='supports')))
            publish(assessment)
        exported=owner.export(git(root,'rev-parse','HEAD'))
        self.assertEqual([],validate(exported,artifact_resolver=resolver))
        self.assertTrue(validate(exported))
        (self.root/'metadata.json').unlink()
        with self.assertRaises(ValueError): owner.export(git(root,'rev-parse','HEAD'))
