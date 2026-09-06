from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

from orbit_research.cli import main
from orbit_research.importers import write_report


class CliTests(unittest.TestCase):
    def test_fresh_representative_sources_all_adapters(self):
        spec = importlib.util.spec_from_file_location('fixtures',Path(__file__).resolve().parents[1]/'examples/make_fixture_sources.py')
        fixture = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fixture)
        with tempfile.TemporaryDirectory() as tmp:
            root = fixture.create(Path(tmp)/'sources')
            before = {str(p):p.read_bytes() for p in root.rglob('*') if p.is_file()}
            for adapter in ['principia','parallax','orrery','astrolabe']:
                output=Path(tmp)/(adapter+'.json')
                stdout=StringIO()
                with redirect_stdout(stdout):
                    code=main(['import',adapter,'--source-root',str(root/adapter),'--repository',adapter,'--dry-run','--output',str(output)])
                self.assertEqual(0,code,stdout.getvalue())
                report=json.loads(output.read_text())
                self.assertTrue(report['source_unchanged'])
                with redirect_stdout(StringIO()):
                    self.assertEqual(0,main(['validate',str(output)]))
            self.assertEqual(before,{str(p):p.read_bytes() for p in root.rglob('*') if p.is_file()})

    def test_json_diagnostic_invalid_input(self):
        with tempfile.TemporaryDirectory() as tmp:
            p=Path(tmp)/'bad.json'
            p.write_text('{broken')
            err=StringIO()
            with redirect_stderr(err):
                self.assertEqual(2,main(['validate',str(p)]))
            self.assertEqual('invalid-input',json.loads(err.getvalue())['error']['code'])


if __name__=='__main__':
    unittest.main()
