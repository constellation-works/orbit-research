from contextlib import redirect_stdout, redirect_stderr
from io import StringIO
import json
from pathlib import Path
import tempfile
import unittest

from orbit_research.cli import main
from orbit_research.orbit_path import task_context


class OrbitPathTests(unittest.TestCase):
    def test_ordinary_cli_task_path_and_explicit_routing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); executable=root/'orbit-fixture'
            # This fixture is an executable protocol peer, not a live Orbit store.
            executable.write_text('''#!/usr/bin/env python3
import json, pathlib, sys
args=sys.argv[1:]
pathlib.Path(__file__).with_suffix('.args').write_text(json.dumps(args))
request=json.loads(args[args.index('--input')+1])
assert args[:3] == ['tool','run','orbit.task.show']
print(json.dumps({'id':request['id'],'workspace':{'id':request['workspace']},'status':'in-progress'}))
''')
            executable.chmod(0o755)
            values=dict(orbit_root=str(root/'authority'),host='remote-fixture',workspace='ws_fixture',task='fixture-task',run='fixture-run',executable=executable)
            context=task_context(**values)
            args=json.loads(executable.with_suffix('.args').read_text())
            self.assertEqual(values['orbit_root'],args[args.index('--root')+1])
            self.assertEqual(dict(id='fixture-task',workspace='ws_fixture',model='codex'),json.loads(args[args.index('--input')+1]))
            self.assertEqual(dict(host='remote-fixture',workspace='ws_fixture',task='fixture-task',run='fixture-run'),context['orbit_link'])
            # A server response from another authority is a failure, not a local retry.
            executable.write_text('#!/usr/bin/env python3\nimport json\nprint(json.dumps({"id":"fixture-task","workspace":{"id":"ws_other"},"status":"in-progress"}))\n')
            with self.assertRaisesRegex(ValueError,'workspace differs'):
                task_context(**values)
            executable.write_text('#!/usr/bin/env python3\nimport json\nprint(json.dumps({"id":"fixture-task","workspace":{"id":"ws_fixture"},"status":"done"}))\n')
            with self.assertRaisesRegex(ValueError,'terminal'):
                task_context(**values)
            values['orbit_root']='relative'
            with self.assertRaisesRegex(ValueError,'absolute'):
                task_context(**values)

    def test_versioned_resource_is_available_without_install_side_effects(self):
        output=StringIO()
        with redirect_stdout(output):
            self.assertEqual(0,main(['resource','--version','1']))
        resource=json.loads(output.getvalue())
        self.assertEqual(1,resource['version'])
        self.assertTrue(resource['skill'].startswith('---\nname: orbit-research-native\n'))
