# 新 VM 验收证据索引

本表把方案第 18 节的场景映射到可复现测试。它是测试入口索引，不将单项测试通过等同于整个场景的完备证明；布局选择、资源审计和性能交付状态见 [实现状态](new-vm-implementation-status.zh-CN.md)。

Rust 集成测试位于 `crates/coflow-core/tests/`，内部存储测试位于 `crates/coflow-core/src/`；C# 测试位于 `runtimes/csharp/Coflow.Runtime.Tests/`。

| 编号 | Rust 入口 | 补充入口与边界 |
| --- | --- | --- |
| V01 | `vm_integration.rs`：`runtime_profiles_link_each_cfd_snapshot_without_changing_success_results`、`fixed_owner_specialization_and_static_templates_are_snapshot_local` | runtime_contract.rs: immutable_image_is_shared_across_thread_owned_instances |
| V02 | `vm_integration.rs`：`unused_direct_calls_preserve_faults_and_divergence`、`range_proven_hoisting_preserves_empty_loop_and_checked_faults` | — |
| V03 | `vm_execution.rs`：`executes_checked_arithmetic_and_float_promotion` | vm_differential.rs: generated_checked_programs_match_independent_oracle_in_both_profiles；runtime/array.rs: packed_numbers_preserve_bits_and_do_not_expose_gc_edges、optional_numbers_keep_presence_and_bits_without_gc_edges |
| V04 | `runtime_contract.rs`：`copied_constant_callables_keep_creation_identity_and_binding` | vm_integration.rs: repeated_captureless_closure_creation_keeps_identity_in_both_profiles |
| V05 | `vm_integration.rs`：`template_collections_preserve_filter_storage_and_short_circuit_reads`、`concatenation_preserves_host_order_in_both_profiles` | — |
| V06 | `vm_integration.rs`：`dimensions_execute_default_for_and_variants_in_declaration_order` | SnapshotTests.cs: ExplicitNoneDimensionRemainsPresentAndDoesNotFallBack |
| V07 | `runtime_contract.rs`：`data_cannot_be_loaded_as_a_record_and_record_keys_span_inheritance` | runtime/fixed.rs: compact_publication_relocates_cycles_and_all_scalar_edges、object_rows_share_typed_layout_across_none_and_host_overrides |
| V08 | `vm_builders.rs`：`rejects_aliases_captures_uninitialized_fields_and_nested_mutation`、`control_transfer_discards_unfrozen_builders` | — |
| V09 | `vm_integration.rs`：`grandparent_captures_survive_multiple_returns_and_collection`、`retained_child_outlives_released_parent_and_old_ids_never_alias_new_values` | vm_builders.rs: builder_and_self_bindings_survive_collection_pressure |
| V10 | `vm_integration.rs`：`paused_outer_frames_keep_arrays_and_closure_captures_across_host_gc_and_reentry` | — |
| V11 | `vm_integration.rs`：`execution_limits_cover_iterations_memory_and_recursive_frames`、`host_only_reentry_shares_depth_and_work_and_releases_failed_boundaries`、`temporary_set_keys_share_budget_and_failed_calls_release_reservations` | runtime/execution.rs: regex_cache_obeys_shared_budget_and_releases_after_failure；资源账目见架构文档第 10 节。 |
| V12 | `vm_control_flow.rs`：`range_entry_liveness_follows_exit_pc_not_descriptor_index`、`iterator_inputs_remain_live_when_output_reuses_an_input_slot` | vm_integration.rs: nested_ranges_and_empty_ranges_preserve_profile_results_after_fusion |
| V13 | `vm_integration.rs`：`out_of_range_external_ids_are_errors_before_compact_encoding`、`missing_reference_in_unreachable_nested_code_prevents_publication` | SnapshotProtocolTests.cs: EveryTruncatedPrefixFailsBeforePublishingANodeGraph; InvalidLengthsEdgesIdentitiesAndPresenceAreRejected; BoundedByteMutationsNeverEscapeAsDecoderOrCollectionExceptions |
| V14 | — | SnapshotTests.cs: OrdinaryReadsRemainManagedAcrossThreadsAndDisposal; EachHostPropertyReadsExactlyOnceIncludingOptionalRecordsAndEnums |
| V15 | — | SnapshotTests.cs: DetachedDataCopiesInputsAndRoundTripsThroughTypedImport；NativeRuntimeTests.cs: FunctionsTemplatesAndHostCallsExecute |
| V16 | — | SnapshotTests.cs: FailedCandidateBuildPreservesOldSnapshotAndExecutionIsThreadAffine; CrossInstanceInputsRejectCapabilitiesButImportPlainContent; OrdinaryReadsRemainManagedAcrossThreadsAndDisposal |
| V17 | `vm_checks.rs`：`check_reports_all_failed_require_calls_and_runs_each_request`、`fault_ends_current_check_and_other_rules_continue`、`check_budget_stops_request_and_records_unfinished_tasks` | — |
| V18 | `vm_builders.rs`：`constructs_and_updates_collections_without_changing_inputs`、`unpublished_self_bindings_cannot_escape_or_execute` | vm_execution.rs: dictionaries_keep_insertion_order_and_normalized_keys |
| V19 | — | SnapshotTests.cs: DetachedDataCopiesInputsAndRoundTripsThroughTypedImport; CrossInstanceInputsRejectCapabilitiesButImportPlainContent |
| V20 | — | coflow-ffi/src/tests.rs: foreign_access_is_rejected_and_finalizer_release_runs_on_creator; creator_thread_exit_releases_unclaimed_runtime_without_leaking_registry_entry；NativeRuntimeTests.cs: HostCyclesAreCollectibleAndLiveRuntimeOwnsItsHost |

## 复现

```powershell
cargo check --workspace
cargo test --workspace
cargo build -p coflow-ffi --release
dotnet test runtimes/csharp/Coflow.Runtime.Tests/Coflow.Runtime.Tests.csproj --configuration Release
```

前两项是仓库普通开发门禁；后两项用于本次原生协议及 C# 专项验收。Unity 独立播放器的复现脚本见实现状态。


源码映射压缩由 `vm/image.rs::source_map_round_trips_runs_and_dense_fault_locations` 验证逐 PC 往返；`vm_integration.rs::cft_and_cfd_faults_point_to_original_utf8_expressions` 验证实际错误定位。内存/GC 观测的 8 个负载逐一断言释放结果并收集后动态存活值归零；布局探针同时测生产对象解码、理想 AoS/SoA 和通用引用间接访问，原始数据随测量报告保存。
