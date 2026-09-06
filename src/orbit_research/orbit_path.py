"""Small explicitly routed client for the existing Orbit task tool; no lifecycle engine."""
import json
from pathlib import Path
import subprocess

from .science import require


def task_context(*, orbit_root, host, workspace, task, run, executable='orbit'):
    """Read an assigned task on the caller-selected host. Never infer authority from cwd.

    orbit_root is the operator-selected authority on host (or on the destination of
    an operator-provided SSH wrapper executable). This function does not route SSH.
    """
    require(Path(orbit_root).is_absolute(), 'explicit absolute Orbit authority root required')
    require(all(isinstance(x, str) and x.strip() for x in (host, workspace, task, run)),
            'explicit host/workspace/task/run required')
    request = dict(id=task, workspace=workspace, model='codex')
    result = subprocess.run([str(executable), 'tool', 'run', 'orbit.task.show', '--root', str(orbit_root),
                             '--input', json.dumps(request)], capture_output=True, text=True, timeout=30, check=True)
    value = json.loads(result.stdout)
    # The registered CLI may wrap its structured output; accept only documented
    # task-shaped results, never a successful process as evidence of a task result.
    if isinstance(value, dict) and isinstance(value.get('result'), dict):
        value = value['result']
    require(isinstance(value, dict) and value.get('id') == task, 'Orbit response does not identify assigned task')
    owner = value.get('workspace', {})
    require(isinstance(owner, dict) and owner.get('id') == workspace, 'Orbit response workspace differs from explicit authority')
    require(not value.get('terminal') and value.get('status') not in {'done','rejected'}, 'assigned task is terminal')
    return dict(task=value, orbit_link=dict(host=host, workspace=workspace, task=task, run=run))
