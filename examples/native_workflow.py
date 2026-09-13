"""Fresh, disposable CLI acceptance workflows. Generates fixture records, never runs science."""
import argparse
from datetime import datetime, timedelta, timezone
from hashlib import sha256
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def digest(value):
    return 'sha256:' + sha256(value).hexdigest()


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args], text=True).strip()


class Workflow:
    def __init__(self, destination, namespace, orbit_link):
        self.root = destination / namespace
        self.root.mkdir()
        self.namespace, self.link = namespace, orbit_link
        self.sequence = 0
        git(self.root, 'init', '-q')
        (self.root/'fixture.py').write_text('# Acceptance metadata only. No scientific computation.\n')
        (self.root/'requests').mkdir()
        git(self.root, 'add', 'fixture.py')
        git(self.root, '-c', 'user.name=Acceptance fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture apparatus')
        self.code = dict(repository=namespace, git_revision=git(self.root, 'rev-parse', 'HEAD'))
        self.scope = 'synthetic-calibration'

    def cli(self, command, *args):
        argv=[os.environ.get('ORBIT_RESEARCH_BINARY', 'orbit-research'), command]
        if command not in {'validate','reconcile'}:
            argv += ['--owner-root', str(self.root), '--repository', self.namespace]
        p=subprocess.run(argv+list(args),capture_output=True,text=True)
        if p.returncode:
            raise RuntimeError(p.stderr or p.stdout)
        return json.loads(p.stdout)

    def append(self, operation, ident, payload, *, prior=None, references=None, kind=None):
        self.sequence += 1
        request=dict(request_id=f'fixture-{self.sequence}',id=ident,scope=self.scope,payload=payload,
                     expected_heads=[prior['revision_id']] if prior else [], reason='Disposable CLI acceptance fixture; no scientific result.',
                     orbit_links=[self.link], references=references or [], limitations=['Synthetic acceptance fixture only.'])
        if kind: request['kind']=kind
        path=self.root/'requests'/f'{self.sequence:02d}-{operation}.json'
        path.write_text(json.dumps(request,indent=2)+'\n')
        result=self.cli(operation,'--request',str(path))
        retry=self.cli(operation,'--request',str(path))
        assert result == retry, 'identical request must be idempotent'
        return result

    def publish(self, record):
        # Only the disposable fixture checkout is committed by this script.
        assert (self.root/'.git').is_dir()
        git(self.root, 'add', 'research/records')
        git(self.root, '-c','user.name=Acceptance fixture','-c','user.email=fixture@example.invalid','commit','-qm','fixture immutable append')
        pin=git(self.root,'rev-parse','HEAD')
        return self.cli('ref','--id',record['id'],'--revision',record['revision_id'],'--source-revision',pin)

    def run(self, nonphysics=False):
        question='Does a fixed assistant evaluator identify planted errors?' if nonphysics else 'Does the synthetic estimator pass null and injected-signal controls?'
        program=self.append('program','R04' if nonphysics else 'control-program',dict(role='program',title='Assistant evaluation fixture' if nonphysics else 'Physics control fixture',question=question))
        prog_ref=self.publish(program)
        claim=self.append('claim','R04/H01' if nonphysics else 'C1',dict(role='hypothesis',statement=question,domain='empirical' if nonphysics else 'model'),references=[prog_ref])
        c_ref=self.publish(claim)
        data=self.append('artifact','seed-plan',dict(role='dataset',availability='available',snapshot_digest=digest(b'fixed fixture seed plan'),locator='fixture:seed-plan',media_type='application/json'))
        d_ref=self.publish(data)
        boundary=(datetime.now(timezone.utc)+timedelta(seconds=.5)).isoformat()
        semantic=dict(question=question,assumptions='Only planted synthetic examples; no claim about nature or deployed assistants.',analysis='Fixed scoring rule.',
                      exclusions='None.',stopping_rule='Complete the enumerated 100 fixtures.',claims=[c_ref],inputs=[d_ref],code=self.code,
                      holdout=dict(digest=data['payload']['snapshot_digest'],information_cutoff='2026-01-01T00:00:00Z',evaluation_not_before=boundary,policy='Freeze seed plan before generating fixture responses.'),
                      design=dict(kind='synthetic',samples=dict(total=100,groups=[dict(name='null',count=50),dict(name='signal',count=50)]),
                                  baseline='Null estimator',controls=['negative','estimator'],resource_budget=dict(planned=100,limit=100,unit='fixture evaluations'),
                                  decision=dict(metric='fixture accuracy',operator='>=',threshold=.9,attainable_min=0,attainable_max=1)))
        protocol=self.append('preregister','R04/E01:protocol' if nonphysics else 'P1',dict(semantic=semantic))
        p_ref=self.publish(protocol)
        time.sleep(max(0,(datetime.fromisoformat(boundary)-datetime.now(timezone.utc)).total_seconds()))
        def run_payload(status, controls, start=None, outputs=None):
            return dict(execution_status=status, controls=controls, control_results={k:controls for k in semantic['design']['controls']},
                        protocol=p_ref,result_artifacts=outputs or [],inputs=[d_ref],code=self.code,
                        environment={'python':sys.version.split()[0],'fixture':True},invocation=['python','fixture.py'],
                        deviations=[],start=start,holdout_digest=semantic['holdout']['digest'])
        start=self.append('begin-run','R04/E01' if nonphysics else 'E1',run_payload('running','not-run'))
        s_ref=self.publish(start)
        output=self.append('artifact','fixture-output',dict(role='result',availability='available',snapshot_digest=digest(b'fixture: criterion met'),locator='fixture:result',media_type='text/plain'))
        out_ref=self.publish(output)
        successful=self.append('record-run','R04/E01' if nonphysics else 'E1',run_payload('completed','passed',s_ref,[out_ref]),prior=start)
        r_ref=self.publish(successful)
        base=dict(claim=c_ref,verdict='supported',inference='confirmatory-primary',controls='passed',basis='scientific-evidence',rationale='Fixture criterion met within synthetic scope only.',evidence=[r_ref],legacy_verdict=None,evidence_summary='supports')
        assessed=self.append('assess','fixture-assessment',base)
        self.publish(assessed)
        failed=self.append('record-run','E-failed',run_payload('failed','failed'))
        f_ref=self.publish(failed)
        narrow={**base,'verdict':'inconclusive','inference':'exploratory','controls':'failed','evidence':[f_ref],'evidence_summary':'inconclusive','rationale':'Failed control; primary confirmation unavailable.'}
        limited=self.append('assess','failed-assessment',narrow)
        self.publish(limited)
        cancelled=self.append('record-run','E-cancelled',run_payload('cancelled','not-run'))
        self.publish(cancelled)
        retired=self.append('retire','R04/H01' if nonphysics else 'C1',claim['payload'],prior=claim,kind='claim',references=[prog_ref])
        self.publish(retired)
        trace=self.cli('trace','--id',claim['id'],'--revision',claim['revision_id'])
        (self.root/'trace.json').write_text(json.dumps(trace,indent=2)+'\n')
        output_path=self.root/'export.json'
        exported=self.cli('export','--source-revision',git(self.root,'rev-parse','HEAD'),'--output',str(output_path))
        assert self.cli('validate',str(output_path))['valid']
        bundle=json.loads(output_path.read_text())
        # Exercise the ordinary manifest reconciliation CLI against exact exported records.
        target_args=[]
        target_dir=self.root/'export-targets'; target_dir.mkdir()
        for n,record in enumerate(bundle['records']):
            path=target_dir/f'{n}.json'; path.write_text(json.dumps(record))
            target_args += ['--target',str(path)]
        manifest=self.root/'manifest.json'; manifest.write_text(json.dumps(bundle['manifests'][0]))
        self.cli('reconcile',str(manifest),*target_args,'--output',str(self.root/'reconciled.json'))
        return dict(owner=str(self.root),export=exported,original_claim=claim['revision_id'],retired_claim=retired['revision_id'],verdicts=['supported','inconclusive'])


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('destination',type=Path,help='new directory outside any scientific owner checkout')
    args=p.parse_args()
    args.destination.mkdir(parents=True,exist_ok=False)
    link=dict(host='fixture-host',workspace='ws_fixture',task='fixture-task',run='fixture-run')
    outputs=[Workflow(args.destination,'physics-fixture',link).run(), Workflow(args.destination,'parallax-fixture',link).run(nonphysics=True)]
    print(json.dumps(dict(fixtures=outputs),indent=2))


if __name__=='__main__': main()
