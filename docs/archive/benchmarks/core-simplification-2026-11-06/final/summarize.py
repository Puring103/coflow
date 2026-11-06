"""Recompute arithmetic medians/ranges from the successful five paired rounds."""
from pathlib import Path
import json,statistics,hashlib
archive=Path(__file__).resolve().parent
results={}
for version in ['baseline','after']:
    results[version]={}
    for round_index in range(1,6):
        engine=archive/f'r{round_index}-{version}-engine.stdout.ndjson'
        meta=json.loads((archive/f'r{round_index}-{version}-engine.meta.json').read_text())
        assert meta['exit_code']==0
        for row in map(json.loads,engine.read_text(encoding='utf-8-sig').splitlines()):
            if 'metric' not in row:continue
            assert len(row['samples_ms'])==3
            assert row['verification']
            results[version].setdefault(row['metric'],[]).append({'round':round_index,'source':engine.name,**row})
        cli_file=archive/f'r{round_index}-{version}-cli.results.json'
        cli=json.loads(cli_file.read_text(encoding='utf-8-sig'))
        assert cli['success']
        for row in cli['metrics']:
            assert row['correctness']=='passed' and len(row['samples_ms'])==3
            key='cli_'+row['metric']
            results[version].setdefault(key,[]).append({'round':round_index,'source':cli_file.name,**row,
                'fixture':{'records':row['records'],'files':row['files'],'fields':row['fields'],
                    'sha256':row['fixture_sha256'],'source_bytes':row['source_bytes']}})
summary=[]
for name, before_rounds in results['baseline'].items():
    after_rounds=results['after'][name]
    assert len(before_rounds)==len(after_rounds)==5
    fixture=before_rounds[0]['fixture']
    for row in before_rounds+after_rounds:assert row['fixture']==fixture,(name,'fixture changed')
    if 'first_record_fields_sha256' in before_rounds[0]:
        checksums={x['first_record_fields_sha256'] for x in before_rounds+after_rounds}
        assert len(checksums)==1,(name,'semantic checksum changed')
    entry={'metric':name,'fixture':fixture,'rounds':5,'samples_per_version':15,'paired_rounds':[]}
    for version, rounds in [('baseline',before_rounds),('after',after_rounds)]:
        samples=[time for row in rounds for time in row['samples_ms']]
        entry[version]={'samples_ms':samples,'median_ms':statistics.median(samples),
            'min_ms':min(samples),'max_ms':max(samples),'sources':[r['source'] for r in rounds]}
    entry['change_percent']=(entry['after']['median_ms']/entry['baseline']['median_ms']-1)*100
    for before,after in zip(before_rounds,after_rounds):
        a,b=statistics.median(before['samples_ms']),statistics.median(after['samples_ms'])
        entry['paired_rounds'].append({'round':before['round'],'baseline_median_ms':a,
            'after_median_ms':b,'change_percent':(b/a-1)*100})
    summary.append(entry)
output={'protocol':'5 serial AB/BA rounds, 1 warmup + 3 measured samples per process; 15 samples per metric per version',
    'comparison':'negative change_percent means faster; descriptive sample statistics, no P95/P99 or memory claims',
    'metrics':summary}
(archive/'summary.json').write_text(json.dumps(output,ensure_ascii=False,indent=2),encoding='utf-8')
lines=['| 场景 | baseline 中位数 [范围] ms | after 中位数 [范围] ms | 变化 |',
       '| --- | ---: | ---: | ---: |']
for entry in summary:
    def cell(v):
        x=entry[v];return f"{x['median_ms']:.3f} [{x['min_ms']:.3f}, {x['max_ms']:.3f}]"
    lines.append(f"| {entry['metric']} | {cell('baseline')} | {cell('after')} | {entry['change_percent']:+.1f}% |")
(archive/'summary-table.md').write_text('\n'.join(lines)+'\n',encoding='utf-8')
print('\n'.join(lines))
