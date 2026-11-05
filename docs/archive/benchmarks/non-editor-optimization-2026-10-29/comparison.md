# 优化前后成对测量

单位：毫秒。汇总值是各独立进程中位数的算术中位数；完整样本、每轮比例和内存指标见 comparison.json。超时不填作实际耗时。

| 场景 | 操作 | 旧版 | 优化版 | 降低比例 | 配对轮数 |
| --- | --- | ---: | ---: | ---: | ---: |
| csharp-dim1-sparse-10k | load_text | 1280.366 | 418.124 | 67.343% | 3 |
| csharp-dim1-sparse-10k | dimension_first_record | 0.101 | 0.051 | 49.455% | 3 |
| csharp-dim1-sparse-10k | dimension_first_all | 315.555 | 57.127 | 81.896% | 3 |
| csharp-dim1-sparse-10k | dimension_cached_for_batch | 4.460 | 1.162 | 73.948% | 3 |
| csharp-dim1-sparse-10k | dimension_combined_first_record | 0.127 | 0.091 | 28.358% | 3 |
| csharp-dim1-sparse-10k | dimension_fallback_first_record | 0.087 | 0.076 | 13.143% | 3 |
| csharp-dim1-sparse-10k | dimension_fallback_cached_batch | 3.795 | 1.180 | 68.897% | 3 |
| csharp-dim1-sparse-10k | dimension_variants_batch | 0.856 | 0.192 | 77.547% | 3 |
| csharp-dim1-sparse-10k | dimension_has_variant_batch | 0.801 | 0.498 | 37.866% | 3 |
| csharp-dim1-sparse-10k | reject_unknown_type_at_end | 385.918 | 388.515 | -0.673% | 3 |
| csharp-dim1-sparse-10k | reject_syntax_at_end | 381.862 | 297.678 | 22.046% | 3 |
| csharp-dim1-sparse-10k | reject_missing_reference | 525.896 | 325.219 | 38.159% | 3 |
| csharp-dim1-sparse-10k | load_after_invalid_input | 1417.766 | 543.957 | 61.633% | 3 |
| csharp-dim10-no-templates-10k | load_text | 6753.157 | 1942.943 | 71.229% | 3 |
| csharp-width100-10k | load_text | 2681.622 | 726.418 | 72.911% | 3 |
| csharp-width100-10k | table_lookup_batch | 1.086 | 0.546 | 49.749% | 3 |
| csharp-width100-10k | direct_property_batch | 0.037 | 0.037 | 0.134% | 3 |
| csharp-width100-10k | table_enumerate | 0.220 | 0.275 | -25.256% | 3 |
