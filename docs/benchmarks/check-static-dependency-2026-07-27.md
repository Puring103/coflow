# Static check dependency benchmark

Date: 2026-07-27

This benchmark validates the production full and incremental check planners, checker execution,
dimension projection fanout, diagnostic-heavy evaluation, inheritance, deduplication, and bounded
task rejection after the static dependency refactor.

## Environment

- CPU: Intel Core i5-13600KF, 14 cores / 20 logical processors
- Memory: 31.8 GiB
- OS/toolchain target: Windows x86_64-pc-windows-msvc
- Rust: 1.96.0 (`ac68faa20`, LLVM 22.1.2)
- Refactored commit: `f6d5ec85`
- Comparison commit: `147c3b1c`
- Sampling: 2 warmups and 7 measured release runs per operation

## Commands

```powershell
cargo bench -p coflow-runtime --features internal-check-bench --bench check_planning
cargo bench -p coflow-runtime --features internal-check-bench --bench check_execution
cargo bench -p coflow-runtime --features internal-check-bench --bench check_limits
```

An execution scenario can be isolated after `--`, for example:

```powershell
cargo bench -p coflow-runtime --features internal-check-bench --bench check_execution -- direct_field:5000
```

## Isolated Results

Isolated runs avoid the thermal and system-load drift observed near the end of the complete
execution suite.

| Scenario | Version | Full plan | Full execute | Incremental plan | Incremental execute | Tasks full/inc | Total speedup |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| direct field, 5k | before refactor | 12.566 ms | 1046.919 ms | 0.008 ms | 0.011 ms | 90001 / 1 | 56355.60x |
| direct field, 5k | refactored | 18.249 ms | 1065.378 ms | 0.015 ms | 0.010 ms | 90001 / 1 | 42329.18x |
| worst fanout, 5k | before refactor | 10.004 ms | 259.159 ms | 9.446 ms | 259.656 ms | 90000 / 90000 | 1.00x |
| worst fanout, 5k | refactored | 14.771 ms | 260.939 ms | 15.564 ms | 261.302 ms | 90000 / 90000 | 1.00x |

The pre-refactor benchmark duplicated planner logic, so its planning columns are historical context
rather than a production-planner comparison. Execution uses the same checker boundary and is
directly comparable.

The comparable full execution regression is about 1.8% for the 5k complex fixture and about 0.7%
for worst-case fanout. Both are below the 15% acceptance threshold. The older 845.7 ms reference
was not reproducible on the current toolchain and machine state; the same-run comparison above is
the relevant regression signal.

## Planning Results

| Scenario | Scale | Full median | Incremental median | Tasks full/inc |
| --- | ---: | ---: | ---: | ---: |
| direct field | 1k | 2.966 ms | 0.002 ms | 18001 / 1 |
| direct field | 5k | 16.826 ms | 0.002 ms | 90001 / 1 |
| direct field | 20k | 65.377 ms | 0.002 ms | 360001 / 1 |
| empty impact | 5k | 15.132 ms | <0.001 ms | 90001 / 0 |
| 100 duplicate changes | 5k | 14.976 ms | 0.021 ms | 90001 / 1 |
| cross-type fanout | 5k | 14.812 ms | 0.631 ms | 90001 / 5000 |
| batch 100 | 5k | 15.855 ms | 0.145 ms | 90001 / 100 |
| dimension variant, 10 variants | 5k | 14.801 ms | 0.002 ms | 55000 / 1 |
| dimension base, 10 variants | 5k | 15.206 ms | 0.004 ms | 55000 / 11 |
| non-dimension field, 10 variants | 5k | 16.219 ms | 0.004 ms | 55000 / 11 |
| inherited field | 5k | 1.141 ms | 0.003 ms | 10000 / 2 |
| worst fanout | 5k | 14.347 ms | 18.217 ms | 90000 / 90000 |

Worst-case incremental planning performs the same work as full planning and is slower in the
complete planning run, but isolated end-to-end execution remains equal to full at 1.00x. This is
within the 1.15x worst-case limit and is the main planner optimization candidate.

## Limit Results

| Scenario | Limit | Plan median | Execute median | Planned | Executed | Rejected | Request diagnostics |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| exact boundary | 5000 | 0.809 ms | 6.810 ms | 5000 | 5000 | 0 | 0 |
| overflow | 4999 | 0.600 ms | 0.347 ms | 0 | 0 | 5000 | 1 |

Overflow rejects the complete request; it does not execute a partial task prefix.

## Conclusions

- Direct, empty, independent, batch, inheritance, dimension, and diagnostic-heavy incremental
  paths all reduce task and work counts as intended.
- Base and non-dimension changes against a dimension-aware statement produce one base task plus
  every related variant task.
- Duplicate impacts are deduplicated before execution.
- Full checker execution has no material regression against the same-run pre-refactor build.
- Worst-case fanout is correctly equivalent to full work and remains the primary planning hotspot.
