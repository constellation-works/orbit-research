from pathlib import Path
import json
import sqlite3
import tempfile
import unittest

from orbit_research import import_source, validate


def tree(root):
    return {str(p.relative_to(root)):p.read_bytes() for p in root.rglob('*') if p.is_file()}


class ParallaxTests(unittest.TestCase):
    def test_nested_frontmatter_claims_experiments_keep_raw_qualifications(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            base=root/'docs/research/R01-fixture'; base.mkdir(parents=True)
            raw_h='---\nresearch_id: R01\nhypothesis_id: H08\nstatus: archived\noutcome: revised\n---\n\n# H08\n\n## Claim\n\nExact multiline claim.\nSecond line.\n\n## Decision history\n\nExploratory failure; no promotion.\n'
            raw_e='---\nresearch_id: R01\nexperiment_id: E02\nhypothesis_id: H09\nstatus: completed\noutcome: revised\npreregistered: true\n---\n\n# E02\n\n## Methodology\n\nAll controls and original terms.\n\n## Outcome\n\nNo independent input control; cannot advance mechanism.\n'
            (base/'H08.md').write_text(raw_h); (base/'E02.md').write_text(raw_e)
            before=tree(root)
            report=import_source(root,'parallax','parallax')
            self.assertEqual([],validate(report))
            self.assertEqual(before,tree(root))
            records=report['candidates']
            claim=next(r for r in records if r['kind']=='claim')
            self.assertEqual('urn:research:parallax:claim:R01%2FH08',claim['id'])
            self.assertEqual('Exact multiline claim.\nSecond line.',claim['payload']['statement'])
            self.assertEqual('archived',claim['legacy']['frontmatter']['status'])
            assessment=next(r for r in records if r['kind']=='assessment')
            self.assertEqual('unknown',assessment['payload']['verdict'])
            self.assertEqual('revised',assessment['payload']['legacy_verdict'])
            protocol=next(r for r in records if r['kind']=='protocol')
            self.assertEqual(raw_e,protocol['payload']['semantic']['source_text'])
            self.assertEqual('historical-unverified',protocol['payload']['freeze'])
            self.assertIsNone(protocol['payload']['frozen_at'])
            run=next(r for r in records if r['kind']=='experiment')
            self.assertEqual('unknown',run['payload']['controls'])
            self.assertEqual(True,run['legacy']['frontmatter']['preregistered'])

    def test_real_researchjournal_schema_wal_and_trade_ids_remain_separate(self):
        # Exact delivered ResearchJournal columns and decision vocabulary; no live journal bytes.
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); (root/'data').mkdir(); path=root/'data/research.sqlite3'
            conn=sqlite3.connect(path)
            try:
                conn.execute('PRAGMA journal_mode=WAL')
                conn.executescript('''CREATE TABLE research_intents (id TEXT PRIMARY KEY, created_at TEXT NOT NULL, venture TEXT NOT NULL, question TEXT NOT NULL, hypothesis TEXT NOT NULL, baseline TEXT NOT NULL, method TEXT NOT NULL, primary_metric TEXT NOT NULL, invalidation TEXT NOT NULL, data_cutoff TEXT NOT NULL);
                CREATE TABLE research_outcomes (experiment_id TEXT PRIMARY KEY REFERENCES research_intents(id), recorded_at TEXT NOT NULL, summary TEXT NOT NULL, decision TEXT NOT NULL CHECK (decision IN ('reject','revise','advance')), artifacts TEXT NOT NULL, limitations TEXT NOT NULL);
                CREATE TABLE trade_intents (id TEXT PRIMARY KEY, hypothesis TEXT);
                CREATE TABLE trade_outcomes (trade_id TEXT PRIMARY KEY, result TEXT);''')
                intent=('same-id','2020-01-01','R04-assistants','Question?','Exact intent','baseline','method','metric','invalidation','cutoff')
                conn.execute('INSERT INTO research_intents VALUES (?,?,?,?,?,?,?,?,?,?)',intent)
                conn.execute('INSERT INTO research_outcomes VALUES (?,?,?,?,?,?)',('same-id','2020-01-02','summary','advance','missing://artifact','Narrow fixture only.'))
                conn.execute('INSERT INTO trade_intents VALUES (?,?)',('same-id','Trading hypothesis'))
                conn.execute('INSERT INTO trade_outcomes VALUES (?,?)',('same-id','P/L not proof'))
                conn.commit()
                before=tree(root)
                report=import_source(root,'parallax','parallax')
                self.assertEqual(before,tree(root))
                self.assertEqual([],validate(report))
                records=report['candidates']
                self.assertEqual(5,len(records))
                self.assertEqual(5,len({r['id'] for r in records}))
                result=next(r for r in records if r['kind']=='experiment' and 'research-journal' in r['id'])
                self.assertEqual('advance',result['legacy']['decision'])
                self.assertEqual('same-id',result['legacy']['experiment_id'])
                self.assertEqual('unknown',result['payload']['controls'])
                self.assertFalse(any(r['kind']=='assessment' for r in records))
                self.assertTrue(any(f['path'].endswith('-wal') for f in report['files']))
                frozen=next(r for r in records if r['kind']=='protocol')
                self.assertEqual(dict(zip(['id','created_at','venture','question','hypothesis','baseline','method','primary_metric','invalidation','data_cutoff'],intent)),frozen['payload']['semantic'])
            finally:
                conn.close()
