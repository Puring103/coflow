"""Five serial AB/BA engine rounds on one confirmed Windows performance CPU."""
from pathlib import Path
import ctypes
from ctypes import wintypes
import datetime
import hashlib
import json
import os
import subprocess
import time

archive = Path(__file__).resolve().parent
source_evidence = archive.parent / 'final'
setup = json.loads((source_evidence / 'setup.json').read_text())
artifacts = json.loads((source_evidence / 'artifacts.json').read_text())
work = Path(setup['isolated_root'])
kernel = ctypes.WinDLL('kernel32', use_last_error=True)
kernel.GetSystemCpuSetInformation.argtypes = [ctypes.c_void_p, wintypes.ULONG,
    ctypes.POINTER(wintypes.ULONG), wintypes.HANDLE, wintypes.ULONG]
kernel.GetSystemCpuSetInformation.restype = wintypes.BOOL
kernel.GetCurrentProcess.restype = wintypes.HANDLE
kernel.SetProcessAffinityMask.argtypes = [wintypes.HANDLE, ctypes.c_size_t]
kernel.SetProcessAffinityMask.restype = wintypes.BOOL
kernel.GetProcessAffinityMask.argtypes = [wintypes.HANDLE,
    ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t)]
kernel.GetProcessAffinityMask.restype = wintypes.BOOL


def affinity(handle):
    process_mask, system_mask = ctypes.c_size_t(), ctypes.c_size_t()
    if not kernel.GetProcessAffinityMask(handle, ctypes.byref(process_mask), ctypes.byref(system_mask)):
        raise ctypes.WinError(ctypes.get_last_error())
    return {'process_mask': hex(process_mask.value), 'system_mask': hex(system_mask.value)}


size = wintypes.ULONG()
kernel.GetSystemCpuSetInformation(None, 0, ctypes.byref(size), None, 0)
buffer = ctypes.create_string_buffer(size.value)
if not kernel.GetSystemCpuSetInformation(buffer, size.value, ctypes.byref(size), None, 0):
    raise ctypes.WinError(ctypes.get_last_error())
topology, offset = [], 0
while offset < len(buffer.raw):
    entry_size = int.from_bytes(buffer.raw[offset:offset + 4], 'little')
    entry_kind = int.from_bytes(buffer.raw[offset + 4:offset + 8], 'little')
    assert entry_size >= 8
    if entry_kind == 0:
        row = buffer.raw[offset + 8:offset + entry_size]
        topology.append({'id': int.from_bytes(row[:4], 'little'),
            'group': int.from_bytes(row[4:6], 'little'), 'logical_processor': row[6],
            'core': row[7], 'last_level_cache': row[8], 'numa_node': row[9],
            'efficiency_class': row[10], 'flags': row[11]})
    offset += entry_size

# 仅约束本次测量进程及其子进程；不改变用户应用、系统调度策略或优先级。
selected = next(row for row in topology if row['group'] == 0 and row['logical_processor'] == 2)
assert selected['efficiency_class'] == max(row['efficiency_class'] for row in topology)
assert selected['efficiency_class'] > min(row['efficiency_class'] for row in topology)
mask = 1 << selected['logical_processor']
runner = kernel.GetCurrentProcess()
before = affinity(runner)
if not kernel.SetProcessAffinityMask(runner, mask):
    raise ctypes.WinError(ctypes.get_last_error())
runner_affinity = affinity(runner)
assert runner_affinity['process_mask'] == hex(mask)
provenance = {'created_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'source_evidence': '../final', 'source_patch_sha256': setup['patch_sha256'],
    'topology_api': 'GetSystemCpuSetInformation', 'topology': topology,
    'selected_cpu': selected, 'runner_affinity_before': before,
    'runner_affinity': runner_affinity, 'artifacts': artifacts,
    'protocol': '5 serial AB/BA engine-only rounds; 1 warmup + 3 samples each; unchanged release binaries'}
(archive / 'setup.json').write_text(json.dumps(provenance, indent=2) + '\n')


def run(version, round_index):
    binary = artifacts[version]['core_simplification_perf']
    assert hashlib.sha256(Path(binary['path']).read_bytes()).hexdigest() == binary['sha256']
    label = f'r{round_index}-{version}-engine'
    args = [binary['path'], 'all']
    env = {'CORE_SAMPLES': '3', 'COFLOW_LOG': '0'}
    meta = {'label': label, 'version': version, 'round': round_index, 'kind': 'engine',
        'args': args, 'cwd': str(work), 'environment_overrides': env,
        'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'binary_sha256': binary, 'runner_affinity': runner_affinity}
    started = time.perf_counter()
    with (archive / f'{label}.stdout.ndjson').open('wb') as out, (archive / f'{label}.stderr.log').open('wb') as err:
        child = subprocess.Popen(args, cwd=work, env={**os.environ, **env}, stdout=out, stderr=err)
        # 子进程启动后立即读取实际mask，逐次确认它继承了runner的固定CPU。
        meta['child_pid'] = child.pid
        meta['child_affinity'] = affinity(wintypes.HANDLE(int(child._handle)))
        assert meta['child_affinity']['process_mask'] == hex(mask)
        (archive / f'{label}.meta.json').write_text(json.dumps(meta, indent=2))
        code = child.wait(timeout=600)
    meta.update(exit_code=code, total_wall_seconds=time.perf_counter() - started,
        ended_utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
    (archive / f'{label}.meta.json').write_text(json.dumps(meta, indent=2))
    print(json.dumps({'completed': label, 'exit_code': code,
        'child_affinity': meta['child_affinity'], 'total_wall_seconds': meta['total_wall_seconds']}), flush=True)
    if code:
        raise RuntimeError(f'{label} failed; raw logs preserved')


for round_index in range(1, 6):
    order = ['baseline', 'after'] if round_index % 2 else ['after', 'baseline']
    for version in order:
        run(version, round_index)
print(json.dumps({'complete': True, 'rounds': 5, 'samples_per_metric_per_version': 15}), flush=True)
