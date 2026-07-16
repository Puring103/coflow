# DataModel schema relationship benchmark

Date: 2026-07-17

This benchmark compares commit `f4cd5bb6` with the Phase 6 canonical schema relationship refactor.
Both revisions were measured on the same Windows machine with the release profile. The reported
values are the median of three warm runs.

## Workload

- 64-level object inheritance chain.
- 512 records of the leaf type.
- 600,000 inheritance root, ancestor, and assignability query operations.
- 200,000 assignable record lookups distributed across the 512 keys.
- 50 complete DataModel index builds.

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

The median complete build cost is approximately 1.330 ms per 512-record model. This is within
normal run-to-run variation of the Phase 6 result (65.916 ms for 50 builds) and is not a material
remaining cost in the measured pipeline. Phase 10 therefore intentionally retains full immutable
DataModel reconstruction for mutations. Shared record nodes, draft fingerprints, partial relation
edge replacement, and `ModelDelta` are not introduced without evidence that their additional
invalidation states and fallback paths would pay for themselves.

Incremental work remains where measurement and behavior justify it: provider reloads are scoped to
affected sources, checker snapshots replace only affected root/round entries, dimension projection
is target-local, and non-structural dimension generation is scoped to assignable changed record
types. Insert, delete, and rename continue to use explicit structural fallbacks.
