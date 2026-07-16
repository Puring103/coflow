# DataModel schema relationship benchmark

Date: 2026-07-17

This benchmark compares commit `f4cd5bb6` with the Phase 6 canonical schema relationship refactor.
Both revisions were measured on the same Windows machine with the release profile. The reported
values are the median of three warm runs.

## Workload

The relationship workload uses:

- 64-level object inheritance chain.
- 512 records of the leaf type.
- 600,000 inheritance root, ancestor, and assignability query operations.
- 200,000 assignable record lookups distributed across the 512 keys.
- 50 complete DataModel index builds.

The representative full-build workload additionally uses:

- 511 direct record refs.
- 511 record spreads.
- Two dimensions with five variants in total.
- 2,560 dimension overlay values.
- 50 complete DataModel builds.

Run with:

```powershell
cargo bench -p coflow-data-model --bench schema_relationships
```

## Results

| Workload | `f4cd5bb6` | Phase 6 | Change |
| --- | ---: | ---: | ---: |
| Schema relationship queries | 421.946 ms | 37.950 ms | 11.1x faster |
| Assignable record lookup | 23.624 ms | 12.700 ms | 1.86x faster |
| Complete model index build | 696.000 ms | 65.916 ms | 10.6x faster |

The representative workload was added during the final spec audit. It is not
compared to `f4cd5bb6` because that revision predates the finalized canonical
dimension overlay and relationship workload; its purpose is to validate the
Phase 10 decision against the complete acceptance shape rather than rewrite
the historical Phase 6 comparison.

The Phase 6 schema stores canonical inheritance roots, ordered ancestor names, and a private
ancestor membership index. DataModel record indexes use `TypeName` directly. Root-type record
lookups skip a redundant assignability query because every member of that inheritance domain is
assignable to its root.

Wall-clock results are supporting evidence and can vary by host. The benchmark fixes workload
sizes and operation counts so later revisions can reproduce the comparison.

## Post-checker and dimension optimization decision

After the checker snapshot and target-scoped dimension work, the same release benchmark was run
three more times on the same machine. The warm-run results were:

| Workload | Run 1 | Run 2 | Run 3 | Median |
| --- | ---: | ---: | ---: | ---: |
| Schema relationship queries | 37.081 ms | 38.408 ms | 37.415 ms | 37.415 ms |
| Assignable record lookup | 13.063 ms | 12.643 ms | 12.887 ms | 12.887 ms |
| 50 complete model builds | 66.482 ms | 66.313 ms | 68.137 ms | 66.482 ms |
| 50 representative model builds | 484.9793 ms | 492.3719 ms | 471.8079 ms | 484.9793 ms |

The median relationship-only build cost is approximately 1.330 ms per 512-record model. The
representative workload costs approximately 9.700 ms per complete build while rebuilding all 511
refs, all 511 spreads, and all 2,560 dimension values. This intentionally dense workload remains
below 10 ms per immutable build and does not establish model materialization as a material remaining
bottleneck after checker and dimension work became target-scoped. Phase 10 therefore intentionally
retains full immutable DataModel reconstruction for mutations. Shared record nodes, draft
fingerprints, partial relation edge replacement, and `ModelDelta` are not introduced without
evidence that their additional invalidation states and fallback paths would pay for themselves.

Incremental work remains where measurement and behavior justify it: provider reloads are scoped to
affected sources, checker snapshots replace only affected root/round entries, dimension projection
is target-local, and non-structural dimension generation is scoped to assignable changed record
types. Insert, delete, and rename continue to use explicit structural fallbacks.
