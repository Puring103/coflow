// 从仓库根目录执行；只校验冻结证据，不构建、不执行被测程序、不更新原始样本。
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';

const repo = process.cwd();
const archive = path.resolve(process.argv[2] ?? 'docs/archive/benchmarks/non-editor-optimization-2026-10-29');
const read = file => JSON.parse(fs.readFileSync(file, 'utf8').replace(/^\uFEFF/u, ''));
const hash = file => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const manifest = read(path.join(archive, 'source-manifest-final.json'));
const issues = [];
function check(file, expected) {
  if (typeof expected !== 'string' || !/^[a-f0-9]{64}$/iu.test(expected)) {
    issues.push({ scope: 'invalid_expected_sha256', file, expected });
    return null;
  }
  const observed = fs.existsSync(file) && fs.statSync(file).isFile() ? hash(file) : null;
  if (observed !== expected.toLowerCase()) issues.push({ file, expected, observed });
  return observed;
}
function comparePaths(scope, actual, expected) {
  const actualSet = new Set(actual), expectedSet = new Set(expected);
  const duplicates = [...new Set(actual.filter((value, index) => actual.indexOf(value) !== index))];
  const missing = [...expectedSet].filter(value => !actualSet.has(value)).sort();
  const unexpected = [...actualSet].filter(value => !expectedSet.has(value)).sort();
  if (duplicates.length || missing.length || unexpected.length) issues.push({ scope, duplicates, missing, unexpected });
}
function filesUnder(directory, prefix = '') {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) return filesUnder(path.join(directory, entry.name), relative);
    if (entry.isFile()) return [relative];
    issues.push({ scope: 'unexpected_snapshot_entry', path: relative });
    return [];
  });
}
const git = args => execFileSync('git', args, { cwd: repo, encoding: 'utf8' });
// 本次源码冻结基于未提交工作区，清单的 base_commit 同时是冻结时的 HEAD。
const currentHead = git(['rev-parse', 'HEAD']).trim();
const currentBranch = git(['branch', '--show-current']).trim();
if (currentHead !== manifest.base_commit) issues.push({ scope: 'git_head_mismatch', expected: manifest.base_commit, observed: currentHead });
if (currentBranch !== manifest.branch) issues.push({ scope: 'git_branch_mismatch', expected: manifest.branch, observed: currentBranch });
const capturedPaths = manifest.files.map(entry => entry.path);
const captured = new Set(capturedPaths);
comparePaths('source_manifest_unique_paths', capturedPaths, [...captured]);
const snapshotPaths = filesUnder(path.join(archive, manifest.snapshot_directory));
comparePaths('source_snapshot_complete_paths', snapshotPaths, [...captured]);
const source = manifest.files.map(entry => ({ ...entry,
  current_sha256: check(path.join(repo, entry.path), entry.sha256),
  snapshot_sha256: check(path.join(archive, manifest.snapshot_directory, entry.path), entry.sha256),
}));
const gitPaths = args => git(args).split('\0').filter(Boolean);
const changed = [...new Set([
  ...gitPaths(['diff', '--name-only', '-z', manifest.base_commit, '--', 'crates', 'src', 'runtimes', 'scripts']),
  ...gitPaths(['ls-files', '--others', '--exclude-standard', '-z', '--', 'crates', 'src', 'runtimes', 'scripts']),
])].filter(file => /^(crates|src|runtimes|scripts)\//u.test(file)).sort();
const missing = changed.filter(file => !captured.has(file));
if (missing.length) issues.push({ scope: 'changed_source_coverage', missing });

// 产物摘要同时绑定最终测量计划和功能验证收据，包含基线、优化版及各自驱动。
const expectedArtifacts = new Map();
const artifactKey = file => path.resolve(file).replaceAll('\\', '/').toLowerCase();
function register(entry, receipt) {
  const key = artifactKey(entry.path);
  const previous = expectedArtifacts.get(key);
  if (previous && previous.sha256.toLowerCase() !== entry.sha256.toLowerCase()) {
    issues.push({ scope: 'conflicting_artifact_receipts', first: previous, second: entry, receipt });
  } else if (previous) previous.receipts.push(receipt);
  else expectedArtifacts.set(key, { path: entry.path, sha256: entry.sha256.toLowerCase(), receipts: [receipt] });
}
const final = path.join(archive, 'final-results');
const finalNames = fs.readdirSync(final);
const finalPlanFiles = finalNames.filter(name => /^plan-.*\.json$/u.test(name));
const finalSpecs = new Map();
function registerSpec(specifications, spec, receipt) {
  const previous = specifications.get(spec.id);
  if (previous && !isDeepStrictEqual(previous, spec)) issues.push({ scope: 'conflicting_process_specification', id: spec.id, receipt, previous, observed: spec });
  else if (!previous) specifications.set(spec.id, spec);
}
if (!finalPlanFiles.length) issues.push({ scope: 'missing_final_plans' });
for (const file of finalPlanFiles) {
  const plan = read(path.join(final, file));
  for (const spec of plan.plans) registerSpec(finalSpecs, spec, `final-results/${file}`);
  for (const entry of plan.payloads) register(entry, `final-results/${file}`);
}
const verification = read(path.join(archive, 'verification.json'));
const plannedArtifacts = new Map(expectedArtifacts);
for (const kind of ['rust_artifacts', 'managed_artifacts']) {
  if (!verification[kind].length) issues.push({ scope: 'missing_verification_artifact_receipts', kind });
  for (const entry of verification[kind]) {
    const planned = plannedArtifacts.get(artifactKey(entry.path));
    // 收据必须证明正式计划使用的同一个产物，分别校验两个版本的并集不能完成绑定。
    if (!planned || planned.sha256 !== entry.sha256.toLowerCase()) {
      issues.push({ scope: 'verification_artifact_not_in_final_plan', kind, receipt: entry, planned: planned ?? null });
    }
    register(entry, 'verification.json');
  }
}
const artifacts = [...expectedArtifacts.values()].map(entry => ({ ...entry, observed_sha256: check(entry.path, entry.sha256) }));

// 从三个固定原始目录重新推导复制集合，不信任 provenance 自报的覆盖范围。
// 选择计划及排除 paused 回执的规则与 consolidate-final.mjs 保持一致。
const expectedSources = [
  { directory: 'final-r4', groups: ['rust', 'model', 'cli'] },
  { directory: 'managed-r6', groups: ['csharp', 'mono'] },
  { directory: 'cli-expanded-r4', groups: ['cli'] },
];
const expectedEvidence = new Map();
const originalSpecs = new Map();
function selectEvidence(file, directory) {
  const original = `${directory}/${file}`;
  const previous = expectedEvidence.get(file);
  // 同一来源文件被多个相同规格引用时只计一次；跨来源同名文件仍拒绝。
  if (previous && previous !== original) issues.push({ scope: 'conflicting_original_evidence_name', file, first: previous, second: original });
  else if (!previous) expectedEvidence.set(file, original);
}
for (const source of expectedSources) {
  const directory = path.join(archive, source.directory);
  const names = fs.readdirSync(directory);
  const selectedPlans = names.filter(name => /^plan-.*\.json$/u.test(name))
    .map(name => ({ name, manifest: read(path.join(directory, name)) }))
    .filter(plan => plan.manifest.plans.some(spec => source.groups.includes(spec.group)));
  if (!selectedPlans.length) issues.push({ scope: 'missing_original_plans', directory: source.directory });
  for (const plan of selectedPlans) {
    if (!plan.manifest.plans.every(spec => source.groups.includes(spec.group))) issues.push({ scope: 'mixed_original_plan_groups', directory: source.directory, plan: plan.name });
    selectEvidence(plan.name, source.directory);
    for (const spec of plan.manifest.plans) {
      registerSpec(originalSpecs, spec, `${source.directory}/${plan.name}`);
      if (!names.includes(`${spec.id}.meta.json`)) issues.push({ scope: 'unfinished_original_process', directory: source.directory, id: spec.id });
      for (const name of names.filter(name => name.startsWith(`${spec.id}.`) && !name.endsWith('.paused.json'))) selectEvidence(name, source.directory);
    }
  }
}
comparePaths('final_plan_spec_coverage', [...finalSpecs.keys()], [...originalSpecs.keys()]);
for (const [id, spec] of originalSpecs) {
  if (finalSpecs.has(id) && !isDeepStrictEqual(finalSpecs.get(id), spec)) issues.push({ scope: 'final_original_specification_mismatch', id });
}
const rawSuffix = /\.(?:meta\.json|spec\.json|results\.json|stdout\.ndjson|stderr\.log|measurements\.ndjson|player\.log)$/u;
const evidencePrefixes = [...new Set([...originalSpecs.keys(), ...finalSpecs.keys()])].map(id => `${id}.`);
const finalEvidenceNames = finalNames.filter(name => !name.endsWith('.paused.json')
  && (/^plan-.*\.json$/u.test(name) || rawSuffix.test(name) || evidencePrefixes.some(prefix => name.startsWith(prefix))));
comparePaths('final_raw_evidence_complete_paths', finalEvidenceNames, [...expectedEvidence.keys()]);

// 每份计划原始文件须同时等于来源映射、清单摘要及逐字节复制后的文件。
const provenance = read(path.join(final, 'evidence-provenance.json'));
comparePaths('provenance_source_directories', provenance.sources.map(entry => entry.directory), expectedSources.map(entry => entry.directory));
for (const expected of expectedSources) {
  const actual = provenance.sources.find(entry => entry.directory === expected.directory);
  if (actual) comparePaths(`provenance_source_groups:${expected.directory}`, actual.groups, expected.groups);
}
comparePaths('provenance_complete_paths', provenance.files.map(entry => entry.file), [...expectedEvidence.keys()]);
const provenanceByFile = new Map();
for (const entry of provenance.files) {
  if (!provenanceByFile.has(entry.file)) provenanceByFile.set(entry.file, entry);
  const expectedSource = expectedEvidence.get(entry.file);
  if (expectedSource !== entry.source) issues.push({ scope: 'provenance_source_mismatch', file: entry.file, expected: expectedSource ?? null, observed: entry.source });
}
const evidence = [...expectedEvidence].map(([file, original]) => {
  const entry = provenanceByFile.get(file);
  if (!entry) return { file, expected_source: original, provenance_missing: true };
  return { file, source: entry.source, expected_source: original, expected_sha256: entry.sha256,
    final_sha256: check(path.join(final, file), entry.sha256),
    original_sha256: check(path.join(archive, original), entry.sha256) };
});
const report = { audited_at: new Date().toISOString(), status: issues.length ? 'failed' : 'passed',
  base_commit: manifest.base_commit, branch: manifest.branch, current_head: currentHead, current_branch: currentBranch,
  source_manifest_sha256: hash(path.join(archive, 'source-manifest-final.json')),
  changed_source_files: changed.length, captured_source_files: source.length, unique_captured_source_files: captured.size,
  snapshot_source_files: snapshotPaths.length, original_plan_specs: originalSpecs.size, final_plan_specs: finalSpecs.size,
  expected_original_evidence_files: expectedEvidence.size, provenance_entries: provenance.files.length, final_raw_evidence_files: finalEvidenceNames.length,
  artifacts_count: artifacts.length, original_evidence_files: evidence.length,
  source, artifacts, evidence, issues };
fs.writeFileSync(path.join(archive, 'verification/final-freeze-audit.json'), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ status: report.status, source: source.length, artifacts: artifacts.length, evidence: evidence.length, issues }));
process.exitCode = issues.length ? 1 : 0;
