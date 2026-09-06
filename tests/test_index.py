from copy import deepcopy
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest
from unittest.mock import patch

from orbit_research.browser import export_browser, local_url
from orbit_research.contract import canonical, digest_bytes, reference, revision_digest
from orbit_research.index import IndexBuildError, project, rebuild, read_index, trace
from orbit_research import Owner
from examples.browser_fixture import create
from test_native import initialize, commit, request, semantic, git


class IndexTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.config = create(self.root / 'fixtures')
        self.db = self.root / 'index.sqlite'

    def test_disposable_deterministic_exact_trace_and_independent_axes(self):
        before = {p:p.read_bytes() for p in (self.root/'fixtures').rglob('*') if p.is_file()}
        result = rebuild(self.config,self.db)
        first = read_index(self.db)
        self.db.unlink(); self.assertEqual(result,rebuild(self.config,self.db))
        self.assertEqual(first,read_index(self.db))
        self.assertEqual(before,{p:p.read_bytes() for p in before})
        self.assertEqual(['astrolabe','orrery','parallax','principia'],first['owners'])
        claim = next(n for n in first['records'] if n['record']['aliases']==['wide-binary-power'])
        chain = trace(self.db,claim['key'])['records']
        self.assertEqual({'claim','program','experiment','protocol','artifact','assessment'},{n['record']['kind'] for n in chain})
        run = next(n for n in chain if n['record']['kind']=='experiment')
        self.assertEqual('completed',run['axes']['execution']); self.assertEqual('failed',run['axes']['controls'])
        self.assertEqual('pending',run['reconciliation'])
        assessment = next(n for n in chain if n['record']['kind']=='assessment')
        self.assertEqual('inconclusive',assessment['axes']['verdict'])
        self.assertEqual('historical',assessment['history']); self.assertEqual('not-current',assessment['confirmation'])
        with sqlite3.connect(self.db) as conn:
            self.assertEqual(len(first['records']),conn.execute('SELECT count(*) FROM records').fetchone()[0])
            self.assertGreater(conn.execute('SELECT count(*) FROM links').fetchone()[0],0)

    def test_invalid_or_interrupted_rebuild_preserves_old_database(self):
        rebuild(self.config,self.db); before=self.db.read_bytes()
        p=self.root/'fixtures/owner-records/00.json'; original=p.read_bytes(); p.write_text('{broken')
        with self.assertRaises(IndexBuildError) as caught: rebuild(self.config,self.db)
        self.assertEqual(str(p),caught.exception.problems[0]['source'])
        self.assertEqual(before,self.db.read_bytes()); p.write_bytes(original)
        for failure in [OSError('disk full'),KeyboardInterrupt()]:
            with patch('orbit_research.index.os.replace',side_effect=failure):
                with self.assertRaises(type(failure)): rebuild(self.config,self.db)
            self.assertEqual(before,self.db.read_bytes())
            self.assertFalse(list(self.root.glob('.research-index-*')))

    def test_source_change_missing_target_and_recovery_never_promote_history(self):
        first=project(self.config)[0]
        source=self.root/'fixtures/principia/source.txt'; original=source.read_bytes(); source.write_text('changed owner source')
        changed=project(self.config)[0]
        self.assertTrue(any('source changed' in reason for n in changed['records'] for reason in n['reasons']))
        self.assertFalse(any(n['confirmation']=='eligible' for n in changed['records']))
        source.write_bytes(original); self.assertEqual(first,project(self.config)[0])
        target=self.root/'fixtures/owner-records/05.json'; target.unlink()
        missing=project(self.config)[0]
        self.assertTrue(missing['unresolved'])
        self.assertTrue(any(e['reason']=='exact target is absent' for n in missing['records'] for e in n['edges']))

    def test_existing_reader_keeps_complete_snapshot_across_atomic_replacement(self):
        rebuild(self.config,self.db)
        reader=sqlite3.connect(self.db)
        try:
            original=reader.execute('SELECT body FROM projection').fetchone()[0]
            (self.root/'fixtures/principia/source.txt').write_text('Changed source')
            rebuild(self.config,self.db)
            self.assertEqual(original,reader.execute('SELECT body FROM projection').fetchone()[0])
            self.assertNotEqual(json.loads(original)['content_digest'],read_index(self.db)['content_digest'])
        finally:
            reader.close()

    def test_conflicting_variants_and_unmanifested_pins_stay_pending(self):
        p=self.root/'fixtures/owner-records/01.json'; r=json.loads(p.read_text())
        r['presentation']['title']='Conflicting presentation for identical semantic pin'
        (p.parent/'duplicate.json').write_text(json.dumps(r))
        result=project(self.config)[0]
        conflict=next(n for n in result['records'] if n['record']['id']==r['id'])
        self.assertEqual(2,len(conflict['variants'])); self.assertEqual('pending',conflict['reconciliation'])
        manifest=p.parent/'manifest-0.json'; manifest.unlink()
        self.assertTrue(any('absent from supplied manifests' in reason for n in project(self.config)[0]['records'] for reason in n['reasons']))

    def test_path_symlink_output_and_media_guards(self):
        with self.assertRaisesRegex(ValueError,'outside every owner'): rebuild(self.config,self.root/'fixtures/principia/index.sqlite')
        (self.root/'escape').symlink_to(self.root/'fixtures/principia',target_is_directory=True)
        with self.assertRaisesRegex(ValueError,'symlink'): rebuild(self.config,self.root/'escape/index.sqlite')
        for value in ['https://evil.invalid/','//evil.invalid/','/../owner/','/%2f/','/x\\evil','/x?y']:
            with self.assertRaises(ValueError): local_url(value)
        config=json.loads(self.config.read_text()); config['media'][0]['path']='../parallax/source.txt'
        config['media'].append(dict(record=config['media'][1]['record'],role='illustration',url='javascript:alert(1)'))
        self.config.write_text(json.dumps(config)); rebuild(self.config,self.db)
        export_browser(self.db,self.root/'site',config_path=self.config)
        exported=json.loads((self.root/'site/index.json').read_text())
        self.assertEqual(['inaccessible','local-navigation','inaccessible'],[m['state'] for m in exported['media']])
        self.assertFalse((self.root/'site/media').exists())
        p=self.root/'fixtures/owner-records/00.json'; original=p.read_bytes(); p.unlink(); (self.root/'secret.json').write_bytes(original); p.symlink_to(self.root/'secret.json')
        with self.assertRaises(IndexBuildError): rebuild(self.config,self.db)

    def test_static_export_inert_imported_scripts_and_snapshot_media(self):
        p=self.root/'fixtures/owner-records/01.json'; r=json.loads(p.read_text())
        r['presentation']['title']='</script><script>window.PWNED=1</script>'
        p.write_text(json.dumps(r)); rebuild(self.config,self.db)
        export_browser(self.db,self.root/'site',config_path=self.config)
        script=(self.root/'site/data.js').read_text()
        self.assertNotIn('</script>',script)
        self.assertEqual('verified-snapshot',json.loads((self.root/'site/index.json').read_text())['media'][0]['state'])
        self.assertFalse((self.root/'site/simulation.html').exists())
        self.assertIn("connect-src 'none'",(self.root/'site/index.html').read_text())
        with self.assertRaisesRegex(ValueError,'must be new'): export_browser(self.db,self.root/'site',config_path=self.config)


class NativeIndexTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(); self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name); self.owner_root=self.root/'owner'; initialize(self.owner_root)
        self.owner=Owner(self.owner_root,'fixture')
        self.clock=patch('orbit_research.native.now',return_value='2026-01-02T00:00:00+00:00'); self.now=self.clock.start(); self.addCleanup(self.clock.stop)
        def publish(r): return reference(self.owner.pin(r['id'],r['revision_id'],commit(self.owner_root)),'resolved')
        self.publish=publish
        self.claim=self.owner.apply('claim',request('C1',dict(role='claim',statement='Synthetic exact property.',domain='model')))
        c=publish(self.claim)
        (self.owner_root/'seed.txt').write_bytes(b'seed'); (self.owner_root/'result.txt').write_bytes(b'result')
        git(self.owner_root,'add','seed.txt','result.txt'); git(self.owner_root,'-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-qm','fixture bytes')
        code=dict(repository='fixture',git_revision=git(self.owner_root,'rev-parse','HEAD'))
        d=publish(self.owner.apply('artifact',request('D1',dict(role='dataset',availability='available',snapshot_digest=digest_bytes(b'seed'),locator='seed.txt',media_type='text/plain'))))
        sem=semantic(c,d,code,boundary='2026-01-02T00:00:01+00:00')
        sem['holdout']['digest']=digest_bytes(b'seed')
        p=publish(self.owner.apply('preregister',request('P1',dict(semantic=sem))))
        self.now.return_value='2026-01-02T00:00:02+00:00'
        payload=dict(execution_status='running',controls='not-run',protocol=p,result_artifacts=[],inputs=[d],code=code,
                     environment={'fixture':True},invocation=['python','code.py'],deviations=[],start=None,holdout_digest=digest_bytes(b'seed'),control_results={'negative':'not-run','estimator':'not-run'})
        start=self.owner.apply('begin-run',request('E1',payload)); s=publish(start)
        output=publish(self.owner.apply('artifact',request('result',dict(role='result',availability='available',snapshot_digest=digest_bytes(b'result'),locator='result.txt',media_type='text/plain'))))
        payload.update(execution_status='completed',controls='passed',start=s,result_artifacts=[output],control_results={'negative':'passed','estimator':'passed'})
        run=publish(self.owner.apply('record-run',request('E1',payload,heads=[start['revision_id']],key='finish')))
        self.assessment=self.owner.apply('assess',request('A1',dict(claim=c,verdict='supported',inference='confirmatory-primary',controls='passed',basis='scientific-evidence',rationale='Synthetic fixture criterion met.',evidence=[run],legacy_verdict=None,evidence_summary='supports')))
        publish(self.assessment)
        self.bundle=self.root/'export.json'; self.save_bundle()
        self.config=self.root/'config.json'; self.config.write_text(json.dumps(dict(schema_version=1,checkouts={'fixture':str(self.owner_root)},documents=[str(self.bundle)])))

    def save_bundle(self):
        self.bundle.write_text(json.dumps(self.owner.export(git(self.owner_root,'rev-parse','HEAD'))))

    def assessments(self):
        return [n for n in project(self.config)[0]['records'] if n['record']['id']==self.assessment['id']]

    def test_native_confirmation_loses_eligibility_on_source_or_data_change_and_recovers(self):
        self.assertTrue(all(n['confirmation']=='eligible' for n in self.assessments()))
        before=self.bundle.read_bytes(); source=self.owner_root/'result.txt'; source.write_bytes(b'changed')
        self.assertTrue(all(n['confirmation']=='not-current' for n in self.assessments()))
        self.assertEqual(before,self.bundle.read_bytes())
        source.write_bytes(b'result'); self.assertTrue(all(n['confirmation']=='eligible' for n in self.assessments()))
        # A valid immutable claim revision supersedes the old one; old verdict is retained.
        self.owner.apply('claim',request('C1',dict(role='claim',statement='Revised exact property.',domain='model'),heads=[self.claim['revision_id']],key='revision2'))
        commit(self.owner_root); self.save_bundle()
        self.assertTrue(all(n['confirmation']=='not-current' for n in self.assessments()))
        self.assertTrue(all(n['axes']['verdict']=='supported' for n in self.assessments()))

    def test_native_missing_exact_target_never_uses_newer_source_snapshot(self):
        bundle=json.loads(self.bundle.read_text())
        assessment=next(r for r in bundle['records'] if r['id']==self.assessment['id'])
        target=assessment['payload']['claim']
        bundle['records']=[r for r in bundle['records'] if not (r['id']==target['id'] and r['provenance']['git_revision']==target['source_revision'])]
        for m in bundle['manifests']: m['references']=[r for r in m['references'] if r['id']!=target['id'] or r['source_revision']!=target['source_revision']]
        self.bundle.write_text(json.dumps(bundle))
        self.assertTrue(all(n['confirmation']=='not-current' for n in self.assessments()))
        self.assertTrue(any(e['reason']=='exact target is absent' for n in self.assessments() for e in n['edges']))

    def test_native_conflicting_heads_block_current_confirmation(self):
        fork=request('C1',dict(role='claim',statement='Competing unreconciled property.',domain='model'),heads=[self.claim['revision_id']],key='fork')
        fork['supersedes']=[]
        self.owner.apply('claim',fork); commit(self.owner_root); self.save_bundle()
        self.assertTrue(all(n['confirmation']=='not-current' for n in self.assessments()))
        self.assertTrue(any('conflicting revision heads' in reason for n in project(self.config)[0]['records'] for reason in n['reasons']))

    def test_opposing_current_assessments_are_visible_conflicts_not_adjudicated(self):
        payload=deepcopy(self.assessment['payload']); payload.update(verdict='refuted',evidence_summary='refutes',rationale='Competing owner assessment of the same fixture result.')
        self.publish(self.owner.apply('assess',request('A2',payload))); self.save_bundle()
        assessments=[n for n in project(self.config)[0]['records'] if n['record']['kind']=='assessment']
        self.assertEqual({'supported','refuted'},{n['axes']['verdict'] for n in assessments})
        self.assertTrue(all(n['confirmation']=='conflicting' for n in assessments))
        self.assertTrue(all(n['reconciliation']=='pending' for n in assessments))
