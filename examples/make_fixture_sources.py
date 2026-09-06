"""Create small synthetic source trees outside this repository; no owning packages needed."""
import json
from pathlib import Path
import sqlite3
import sys


def create(destination):
    root = Path(destination)
    root.mkdir(parents=True, exist_ok=False)

    def put(repo, path, value):
        p = root / repo / path
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(value if isinstance(value, str) else json.dumps(value, indent=2)+'\n')

    put('principia', 'theory/calibration/claims.json', {
        'doc':'calibration', 'title':'Synthetic control calibration', 'status':'retired',
        'claims':[{'id':'control-null','kind':'model-property','status':'mixed',
                   'claim':'The synthetic zero-effect control stays below the fixed threshold.',
                   'evidence':'Execution completed, but the negative control failed.',
                   'control_ran':True, 'control':'failed', 'limitation':'No inference about nature.'}]})
    put('principia', 'gates/control.json', {'id':'control','control':'zero injected effect',
        'kill':'absolute error >= 0.04','decision':'Failed control leaves primary inference unresolved.'})
    put('parallax', 'docs/register.md', '# Synthetic empirical program\n\n| ID | Statement | Comparator |\n| --- | --- | --- |\n| R1 | Market measurement program | None |\n| H1 | Forecast lift survives measured costs. | Constant forecast |\n| E1 | Proposed walk-forward test | No outcome yet |\n')
    db = root / 'parallax/data/journal.sqlite'
    db.parent.mkdir()
    with sqlite3.connect(db) as conn:
        conn.executescript('CREATE TABLE trade_intents(id TEXT PRIMARY KEY, created_at TEXT, hypothesis TEXT, entry_rule TEXT, exit_rule TEXT, invalidation TEXT, size TEXT, expected_edge_bps REAL); CREATE TABLE trade_outcomes(trade_id TEXT PRIMARY KEY, recorded_at TEXT, fill TEXT, fees TEXT, slippage_bps REAL, result TEXT);')
        conn.execute('INSERT INTO trade_intents VALUES (?,?,?,?,?,?,?,?)',('intent-1','2020-01-01T00:00:00Z','Forecast lift survives costs.','fixed entry','fixed exit','no lift','unit',2.0))
        conn.execute('INSERT INTO trade_outcomes VALUES (?,?,?,?,?,?)',('intent-1','2020-01-02T00:00:00Z','one fill','one fee',1.0,'Negative after costs.'))
    put('orrery', 'lab/sims/control/sim.json', {'slug':'control','title':'Synthetic control','kind':'py','status':'retired'})
    put('orrery', 'lab/sims/control/assets/results.json', {'execution':'completed','decision':{'control':'failed','verdict':'inconclusive'},'realizations':[{'run_id':'R0','error':0.05}]})
    put('astrolabe', 'data/processed/derived/example.json', {'name':'example','kind':'derived','source':'analysis.example','query':{'synthetic':True},'fetched_at':'2020-01-01T00:00:00Z','n_rows':0,'columns':['x'],'lineage':[{'dataset':'parent','fetched_at':'2019-01-01T00:00:00Z'},{'dataset':None,'note':'not persisted'}]})
    return root


if __name__ == '__main__':
    print(create(sys.argv[1]))
