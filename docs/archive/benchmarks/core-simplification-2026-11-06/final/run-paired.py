"""Frozen release executables, serial AB/BA rounds; all raw outputs retained."""
from pathlib import Path
import subprocess, json, os, datetime, time, hashlib, shutil

archive=Path(__file__).resolve().parent
setup=json.loads((archive/'setup.json').read_text())
work=Path(setup['isolated_root'])
artifacts=json.loads((archive/'artifacts.json').read_text())
node=shutil.which('node')
assert node
rounds=5
samples=3

def digest(path): return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def run(label, args, version, round_index, kind, env):
    assert digest(artifacts[version]['core_simplification_perf']['path']) == artifacts[version]['core_simplification_perf']['sha256']
    assert digest(artifacts[version]['coflow']['path']) == artifacts[version]['coflow']['sha256']
    meta={'label':label,'version':version,'round':round_index,'kind':kind,'args':args,'cwd':str(work),
          'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
          'environment_overrides':env,'binary_sha256':artifacts[version]}
    (archive/f'{label}.meta.json').write_text(json.dumps(meta,indent=2))
    before=time.perf_counter()
    with (archive/f'{label}.stdout.ndjson').open('wb') as out,(archive/f'{label}.stderr.log').open('wb') as err:
        process=subprocess.run(args,cwd=work,env={**os.environ,**env},stdout=out,stderr=err,timeout=600)
    meta.update(exit_code=process.returncode,total_wall_seconds=time.perf_counter()-before,
          ended_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
    (archive/f'{label}.meta.json').write_text(json.dumps(meta,indent=2))
    print(json.dumps({'completed':label,'exit_code':process.returncode,'total_wall_seconds':meta['total_wall_seconds']}),flush=True)
    if process.returncode:raise RuntimeError(f'{label} failed; raw logs preserved')

for round_index in range(1,rounds+1):
    order=['baseline','after'] if round_index%2 else ['after','baseline']
    for version in order:
        label=f'r{round_index}-{version}-engine'
        run(label,[artifacts[version]['core_simplification_perf']['path'],'all'],version,round_index,'engine',
            {'CORE_SAMPLES':str(samples),'COFLOW_LOG':'0'})
    for version in order:
        label=f'r{round_index}-{version}-cli'
        run(label,[node,str(archive/'cli-matrix.mjs'),'--cli',artifacts[version]['coflow']['path'],
            '--case','batch_same_file,batch_cross_file','--records','10000','--files','10','--fields','20',
            '--samples',str(samples),'--warmups','1','--batch','1000','--variants','0','--topology','none',
            '--timeout-ms','300000','--output',str(archive/f'{label}.results.json')],version,round_index,'cli',{'COFLOW_LOG':'0'})
print(json.dumps({'complete':True,'rounds':rounds,'samples_per_metric_per_version':rounds*samples}),flush=True)
