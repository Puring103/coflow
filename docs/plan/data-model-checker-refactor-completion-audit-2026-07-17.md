# DataModel and Checker Refactor Completion Audit

- Date: 2026-07-17
- Specification: `docs/plan/data-model-checker-refactor-plan-2026-07-16.md`
- Chinese specification: `docs/plan/data-model-checker-refactor-plan-2026-07-16.zh-CN.md`
- Base branch: `v0.8`
- Implementation branch: `codex/data-model-checker-refactor-plan`
- Pull request: `#18`

## Phase Audit

| Phase | Outcome | Evidence |
| --- | --- | --- |
| 0. Baseline observability | Complete | `ProjectExecutionStats`, `IncrementalFallbackReason`, checker execution statistics, and deterministic runtime tests; commits `8e006ca4` and `8b071dab`. |
| 1. Value type naming | Complete | `CftValueType` and `CftField.value_type`; commit `8e0a8c57`. |
| 2. Compiler inference unification | Complete | `InferredType::Value(CftValueType)` and no `CheckedType`; commit `0d793074`. |
| 3. Typed record identity | Complete | `RecordCoordinate(TypeName, RecordKey)` in `coflow-data-model`; commit `464826f7`. |
| 4. Shared value semantics | Complete | `validate_value_for_schema` shared by model build and mutation, plus the conformance matrix; commit `c8795f1d`. |
| 5. Ingest/model boundaries | Complete | `ingest`, `build`, `model`, `indexes`, `dependencies`, `semantics`, and diagnostics ownership split; commit `f4cd5bb6`. |
| 6. Canonical schema relationships | Complete | `CftSchema` owns inheritance roots, ancestors, and assignability; DataModel owns only record/relation indexes; commit `e2e6a175`. |
| 7. Checker value/API consolidation | Complete | Borrowed `EvalValue`, `TemporaryValue`, lazy collections, and the single `run_checks` request/output entry; commit `0350e25f`. |
| 8. Checker snapshot ownership | Complete | Per-`CheckRoot`/`CheckRound` logical diagnostics and read dependencies in `CheckSnapshot`; commit `25ca27e4`. |
| 9. Dimension optimization | Complete | Generation-owned `DimensionRuntimePlan`, target-local projection, and impact-scoped generation/reload; commit `3b30fd32`. |
| 10. Incremental DataModel delta | Deliberately not activated | The phase is conditional. The expanded representative benchmark remains below 10 ms per complete 512-record build with 511 refs, 511 spreads, two dimensions, five variants, and 2,560 overlay values. Shared nodes, fingerprints, relation replacement, and `ModelDelta` are therefore not justified; commits `fd676c0b` and `2f42c1d7`. |

## Completion Criteria

1. Legacy schema/checker symbols are removed.

   `CftSchemaTypeRef`, `CheckedType`, `CfdCheckExt`, `CfdTypeId`, `CfdDomainId`,
   `CfdDomainIndex`, `CfdPolymorphicIndex`, and `inheritance_index` have no matches under
   `crates/`, `editors/`, or `src/`.

2. Successful schema values use one canonical type tree.

   `coflow-cft/src/schema/value_type.rs` defines `CftValueType`. Compiler-only states wrap it as
   `InferredType::Value`; successful DataModel and checker paths consume the same tree.

3. Build and mutation share value semantics.

   `coflow-data-model/src/semantics/validation.rs` owns `validate_value_for_schema`.
   `coflow-data-model/tests/value_semantics.rs` runs source-build and mutation cases through the
   same semantic rule matrix.

4. Source drafts are explicit ingest IR.

   Provider output uses `LoadedRecordDraft`, `LoadedValueDraft`, and `DimensionValueDraft` under
   `coflow-data-model/src/ingest/`. Raw string names remain confined to this draft boundary.

5. DataModel has no duplicate schema relationship model.

   `CftSchema` owns inheritance roots, ordered ancestors, descendants, and assignability.
   DataModel indexes canonical `TypeName` and `RecordKey` values without numeric type/domain IDs.

6. Record identity is generation-correct.

   `RecordCoordinate` is the stable typed business identity. `CfdRecordId` is documented and used
   only as a generation-local dense index; snapshots stabilize it before crossing generations.

7. Checker has borrowed values and one execution entry.

   `coflow-checker` exports `run_checks(schema, model, CheckRequest) -> CheckOutput` as its only
   execution function. Model scalar values are borrowed; temporary evaluator scalars and lazy
   collection cursors are separate internal representations.

8. Checker owns incremental snapshot merge behavior.

   `CheckSnapshot` stores logical diagnostics and `reads_from` per
   `CheckRoot { RecordCoordinate, CheckRound }`; runtime retains only physical origin mapping.

9. Subset dimension checks are target-local.

   `DimensionRoundView` starts empty and records projection only when a target field, nested field,
   or referenced record is accessed. `dimension_projected_records` is asserted in checker and
   runtime tests.

10. Dimension planning is generation-bound and impact-scoped.

    `ProjectSession` owns one `Arc<DimensionRuntimePlan>`. Non-structural mutation selects fields by
    changed-record assignability; insert/delete/rename use explicit structural fallback.

11. Incremental work and fallback reasons are observable.

    `ProjectExecutionStats` exposes every counter listed in Phase 0. The stable
    `IncrementalFallbackReason` enum matches the specification, and tests distinguish record insert,
    delete, and rename fallbacks.

12. Incremental and fresh results are equivalent.

    Checker snapshot tests compare incremental and fresh snapshots. Runtime mutation tests compare
    dependent-record diagnostics, dimension mutation diagnostics, dimension-reference diagnostics,
    and editor spread-write diagnostics against fresh full sessions.

13. Compatibility contracts remain unchanged.

    The workspace suites cover CFT/CFD syntax, project configuration, diagnostic codes and spans,
    CLI human/JSON output, editor bindings and DTOs, JSON/MessagePack export, C# output, all loader
    and writer providers, mutation transactions/compensation, spread provenance, and dimension
    missing-versus-null behavior. No golden output or TypeScript binding changed as part of the
    refactor.

14. Repository checks pass.

    The final release gate is run from the worktree root after this audit:

    ```powershell
    pwsh scripts/sync-skill-references.ps1
    pwsh scripts/sync-skill-references.ps1 -Check
    cargo check --workspace
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    ```

## Performance Decision

The deterministic benchmark workload and measurements are recorded in
`docs/benchmarks/data-model-schema-relationships-2026-07-17.md`.

- Relationship-only median: 66.482 ms for 50 builds, about 1.330 ms/build.
- Representative median: 484.9793 ms for 50 builds, about 9.700 ms/build.
- Representative shape: 512 records, 511 refs, 511 spreads, two dimensions, five variants, and
  2,560 dimension values.

The implementation therefore keeps immutable full DataModel reconstruction while making provider
reload, checker execution, dimension projection, and dimension regeneration genuinely incremental.
This is the conditional Phase 10 outcome specified by the plan, not an unreported fallback.
