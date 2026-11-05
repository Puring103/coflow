# 优化前后成对测量

单位：毫秒。汇总值是各独立进程中位数的算术中位数；完整样本、每轮比例和内存指标见 comparison.json。超时不填作实际耗时。

| 场景 | 操作 | 旧版 | 优化版 | 降低比例 | 配对轮数 |
| --- | --- | ---: | ---: | ---: | ---: |
| cli-common-10k | data_get | 404.078 | 394.085 | 2.473% | 1 |
| cli-common-10k | check_rules | 475.134 | 476.618 | -0.312% | 1 |
| cli-common-10k | set_field | 42383.319 | 2265.053 | 94.656% | 1 |
| cli-common-10k | set_field_rules | 42419.344 | 2334.423 | 94.497% | 1 |
| cli-common-10k | set_noop | 43186.577 | 2209.630 | 94.884% | 1 |
| cli-common-10k | set_preview | 42943.513 | 2230.349 | 94.806% | 1 |
| cli-common-10k | batch_same_file | 86147.285 | 2393.738 | 97.221% | 1 |
| cli-common-10k | batch_cross_file | 86428.126 | 2671.834 | 96.909% | 1 |
| csharp-dim1-sparse-10k | load_text | 1256.726 | 405.811 | 67.709% | 1 |
| csharp-dim1-sparse-10k | dimension_first_record | 0.128 | 0.051 | 60.376% | 1 |
| csharp-dim1-sparse-10k | dimension_first_all | 338.845 | 55.609 | 83.589% | 1 |
| csharp-dim1-sparse-10k | dimension_cached_for_batch | 4.300 | 0.977 | 77.267% | 1 |
| csharp-dim1-sparse-10k | dimension_combined_first_record | 0.172 | 0.094 | 45.681% | 1 |
| csharp-dim1-sparse-10k | dimension_fallback_first_record | 0.102 | 0.071 | 30.469% | 1 |
| csharp-dim1-sparse-10k | dimension_fallback_cached_batch | 3.866 | 0.962 | 75.119% | 1 |
| csharp-dim1-sparse-10k | dimension_variants_batch | 0.816 | 0.190 | 76.776% | 1 |
| csharp-dim1-sparse-10k | dimension_has_variant_batch | 0.823 | 0.500 | 39.249% | 1 |
| csharp-dim1-sparse-10k | reject_unknown_type_at_end | 341.249 | 385.598 | -12.996% | 1 |
| csharp-dim1-sparse-10k | reject_syntax_at_end | 321.632 | 292.360 | 9.101% | 1 |
| csharp-dim1-sparse-10k | reject_missing_reference | 505.463 | 335.278 | 33.669% | 1 |
| csharp-dim1-sparse-10k | load_after_invalid_input | 1423.270 | 555.787 | 60.950% | 1 |
| csharp-scalar20-100k | load_text | 5158.562 | 2762.640 | 46.446% | 1 |
| csharp-scalar20-100k | read_files_and_load | 4774.391 | 2481.538 | 48.024% | 1 |
| csharp-scalar20-100k | reload_with_old_result_alive | 5638.972 | 3013.741 | 46.555% | 1 |
| csharp-scalar20-100k | table_lookup_batch | 1.778 | 1.731 | 2.671% | 1 |
| csharp-scalar20-100k | direct_property_batch | 0.037 | 0.037 | 1.075% | 1 |
| csharp-scalar20-100k | table_enumerate | 1.740 | 1.808 | -3.910% | 1 |
| csharp-scalar20-100k | reject_unknown_type_at_end | 2279.631 | 1851.612 | 18.776% | 1 |
| csharp-scalar20-100k | reject_syntax_at_end | 2267.222 | 1758.375 | 22.444% | 1 |
| csharp-scalar20-100k | load_after_invalid_input | 4802.320 | 2396.249 | 50.102% | 1 |
| rust-structures-literal-100k | insert_record | 6004.336 | 673.001 | 88.791% | 1 |
| rust-structures-literal-100k | delete_record | 6037.682 | 665.888 | 88.971% | 1 |
| rust-structures-literal-100k | rename_record | 6015.912 | 723.680 | 87.971% | 1 |
| rust-structures-literal-100k | transfer_record | 6002.566 | 651.345 | 89.149% | 1 |
