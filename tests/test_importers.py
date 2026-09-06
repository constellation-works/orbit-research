from copy import deepcopy
from pathlib import Path
import json
import sqlite3
import subprocess
import tempfile
import unittest

from orbit_research import import_source, validate
from orbit_research.importers import file_digest, write_report


def write(root, name, data):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(data if isinstance(data, str) else json.dumps(data), encoding='utf-8')
    return path


def tree(root):
    return {str(p.relative_to(root)):p.read_bytes() for p in root.rglob('*') if p.is_file()}


def claims():
    return dict(doc='model',title='Model',status='retired',claims=[
        dict(id='C1',claim='Exact mathematical claim ≥ 0.',kind='model-property',status='mixed',
             evidence='Primary controls failed.',control_ran=False,unknown_field={'keep':None}),
        dict(id='C2',claim='Conditional proposition.',kind='nature',status='conditional')])


class ImportTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name) / 'source'
        self.root.mkdir()

    def test_principia_preserves_every_field_and_retirement(self):
        raw = claims()
        write(self.root,'theory/model/claims.json',raw)
        write(self.root,'theory/model/evidence-ledger.md','# Historical ledger\nExact failed-control verdict.\n')
        write(self.root,'gates/control.json',dict(id='G1',control='failed',kill='predeclared threshold'))
        before = tree(self.root)
        report = import_source(self.root,'principia','principia')
        self.assertEqual([], validate(report))
        self.assertEqual(before,tree(self.root))
        c = next(r for r in report['candidates'] if r['kind']=='claim')
        self.assertEqual(raw['claims'][0],c['legacy'])
        self.assertEqual(raw['claims'][0]['claim'],c['payload']['statement'])
        self.assertEqual('retired', next(r for r in report['candidates'] if r['kind']=='program')['activity'])
        self.assertEqual(['inconclusive','conditional'],[r['payload']['verdict'] for r in report['candidates'] if r['kind']=='assessment'])
        self.assertTrue(all(r['payload']['freeze']=='historical-unverified' for r in report['candidates'] if r['kind']=='protocol'))
        self.assertEqual(report['counts']['discovered'],report['counts']['mapped']+report['counts']['exceptions'])

    def test_collisions_are_excepted_not_overwritten(self):
        for name in ['a','b']:
            write(self.root,f'theory/{name}/claims.json',claims())
        report = import_source(self.root,'principia','principia')
        self.assertEqual([], validate(report))
        self.assertEqual([], report['candidates'])
        self.assertEqual(6,sum(any(e['code']=='identity-collision' for e in i['exceptions']) for i in report['inventory']))

    def test_malformed_and_ambiguous_json_accounted(self):
        write(self.root,'theory/a/claims.json','{"claims": [], "claims": []}')
        write(self.root,'theory/b/claims.json',dict(claims=[None,{'claim':'missing id'}]))
        write(self.root,'theory/c/claims.json','{"claims": [NaN]}')
        report = import_source(self.root,'principia','principia')
        self.assertEqual([], validate(report))
        codes = [e['code'] for i in report['inventory'] for e in i['exceptions']]
        self.assertEqual(2,codes.count('read-error'))
        self.assertEqual(2,codes.count('malformed-claim'))

    def test_parallax_h_e_r_prose_exact_and_uninvented(self):
        prose = '# Register\n| H1 | A forecast improves after costs. | baseline |\n| E1 | proposed test | no result |\n| R1 | empirical program |\n## H1–H2: discussion\n'
        write(self.root,'docs/register.md',prose)
        report=import_source(self.root,'parallax','parallax')
        self.assertEqual([],validate(report))
        h=next(r for r in report['candidates'] if r['kind']=='claim')
        self.assertEqual('hypothesis',h['payload']['role'])
        self.assertEqual('empirical',h['payload']['domain'])
        self.assertEqual('A forecast improves after costs.',h['payload']['statement'])
        self.assertFalse(any(r['kind']=='experiment' for r in report['candidates']))
        self.assertTrue(any(i['raw']==prose for i in report['inventory']))
        self.assertEqual(2,sum(any(e['code']=='unmapped-register-row' for e in i['exceptions']) for i in report['inventory']))

    def make_journal(self, wal=False):
        path=self.root/'data/journal.sqlite'
        path.parent.mkdir()
        conn=sqlite3.connect(path)
        if wal:
            conn.execute('PRAGMA journal_mode=WAL')
        conn.executescript('CREATE TABLE trade_intents(id TEXT PRIMARY KEY, created_at TEXT, hypothesis TEXT, entry_rule TEXT, exit_rule TEXT, invalidation TEXT, size TEXT, expected_edge_bps REAL); CREATE TABLE trade_outcomes(trade_id TEXT, recorded_at TEXT, fill TEXT, fees TEXT, slippage_bps REAL, result TEXT); CREATE TABLE extras(id INTEGER, payload BLOB);')
        conn.execute('INSERT INTO trade_intents VALUES (?,?,?,?,?,?,?,?)',('t1','2020-01-01','Exact forecast','entry','exit','invalid','one',2.0))
        conn.execute('INSERT INTO trade_outcomes VALUES (?,?,?,?,?,?)',('t1','2020-01-02','fill','fees',1.0,'profit is not proof'))
        conn.execute('INSERT INTO extras VALUES (?,?)',(1,b'\x00\xff'))
        conn.commit()
        return conn,path

    def test_sqlite_bytes_unchanged_and_every_row_inventoried(self):
        conn,path=self.make_journal()
        conn.close()
        before=tree(self.root)
        report=import_source(self.root,'parallax','parallax')
        self.assertEqual([],validate(report))
        self.assertEqual(before,tree(self.root))
        self.assertEqual(4,report['counts']['discovered'])
        self.assertEqual(2,len(report['candidates']))
        self.assertEqual('completed',next(r for r in report['candidates'] if r['kind']=='experiment')['payload']['execution_status'])
        self.assertFalse(any(r['kind']=='assessment' for r in report['candidates']))
        self.assertEqual(file_digest(path),report['files'][0]['sha256'])

    def test_live_wal_rows_seen_without_source_sqlite_writes(self):
        conn,path=self.make_journal(wal=True)
        try:
            before=tree(self.root)
            report=import_source(self.root,'parallax','parallax')
            self.assertEqual([],validate(report))
            self.assertEqual(before,tree(self.root))
            self.assertEqual(2,len(report['candidates']))
        finally:
            conn.close()

    def test_unknown_sqlite_tables_and_rollback_journal(self):
        conn,path=self.make_journal()
        conn.close()
        Path(str(path)+'-journal').write_bytes(b'hot')
        before=tree(self.root)
        report=import_source(self.root,'parallax','parallax')
        self.assertEqual([],validate(report))
        self.assertEqual(before,tree(self.root))
        self.assertEqual('sqlite-journal',report['inventory'][0]['exceptions'][0]['code'])

    def test_astrolabe_lineage_and_missing_snapshot(self):
        meta=dict(name='derived',kind='derived',source='analysis.crossmatch',query={'ra':180},fetched_at='2020-01-01',n_rows=1,columns=['x'],lineage=[{'dataset':None,'note':'never persisted'},{'dataset':'parent','fetched_at':'2019-01-01'}])
        p=write(self.root,'data/processed/derived/derived.json',meta)
        report=import_source(self.root,'astrolabe','astrolabe')
        self.assertEqual([],validate(report))
        rec=report['candidates'][0]
        self.assertEqual('missing',rec['payload']['availability'])
        self.assertIsNone(rec['payload']['snapshot_digest'])
        self.assertEqual(meta,rec['legacy'])
        p.with_suffix('.parquet').write_bytes(b'synthetic bytes, never a private dataset')
        before=tree(self.root)
        refreshed=import_source(self.root,'astrolabe','astrolabe')
        self.assertEqual([],validate(refreshed))
        self.assertEqual(before,tree(self.root))
        self.assertEqual(rec['id'],refreshed['candidates'][0]['id'])
        self.assertNotEqual(rec['revision_id'],refreshed['candidates'][0]['revision_id'])

    def test_orrery_catalog_not_execution_results_preserved(self):
        write(self.root,'lab/sims/test/sim.json',dict(slug='test',title='Test',status='retired',provenance={'revision':'legacy-short'}))
        results=dict(decision={'controls':'failed','verdict':'unresolved'},realizations=[{'run_id':'R0','value':1},{'run_id':'R1','value':2}])
        write(self.root,'lab/sims/test/assets/results.json',results)
        report=import_source(self.root,'orrery','orrery')
        self.assertEqual([],validate(report))
        self.assertFalse(any(r['kind'] in {'assessment','experiment'} for r in report['candidates']))
        self.assertEqual(4,report['counts']['discovered'])
        self.assertTrue(any(i['raw']==results for i in report['inventory']))

    def test_pinning_and_dirty_file_provenance(self):
        p=write(self.root,'theory/m/claims.json',claims())
        def git(*args):
            return subprocess.check_output(['git','-C',str(self.root),*args],text=True).strip()
        git('init','-q')
        git('add','theory/m/claims.json')
        git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid','commit','-qm','synthetic source')
        head=git('rev-parse','HEAD')
        first=import_source(self.root,'principia','principia',expected_revision=head)
        self.assertEqual([],validate(first))
        self.assertFalse(first['files'][0]['working_tree'])
        p.write_text(p.read_text()+'\n')
        second=import_source(self.root,'principia','principia',expected_revision=head)
        self.assertTrue(second['files'][0]['working_tree'])
        self.assertEqual(first['files'][0]['blob_oid'],second['files'][0]['blob_oid'])
        self.assertNotEqual(first['files'][0]['sha256'],second['files'][0]['sha256'])
        with self.assertRaisesRegex(ValueError,'revision mismatch'):
            import_source(self.root,'principia','principia',expected_revision='a'*40)

    def test_output_and_source_symlink_guards(self):
        write(self.root,'input.json',{})
        with self.assertRaises(ValueError):
            write_report({},self.root/'report.json',[self.root])
        outside=Path(self.tmp.name)/'outside.json'
        outside.write_text('private')
        (self.root/'leak.json').symlink_to(outside)
        with self.assertRaises(ValueError):
            import_source(self.root,'principia','principia',selected=['leak.json'])
        with self.assertRaises(ValueError):
            import_source(self.root,'principia','principia',selected=['../outside.json'])
        output=Path(self.tmp.name)/'report.json'
        output.symlink_to(self.root/'input.json')
        with self.assertRaises(ValueError):
            write_report({},output,[self.root])
        with self.assertRaises(FileExistsError):
            write_report({},outside,[self.root])
        with self.assertRaises(ValueError):
            import_source(self.root,'principia','principia',selected=['.orbit/tasks.json'])

    def test_report_accounting_is_validated(self):
        write(self.root,'theory/m/claims.json',claims())
        report=import_source(self.root,'principia','principia')
        broken=deepcopy(report)
        broken['counts']['mapped']+=1
        self.assertTrue(validate(broken))
        broken=deepcopy(report)
        broken['inventory']=[]
        self.assertTrue(validate(broken))
        broken=deepcopy(report)
        broken['aliases']=[]
        self.assertTrue(validate(broken))


if __name__=='__main__':
    unittest.main()
