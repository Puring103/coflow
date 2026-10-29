# 优化前后成对测量

单位：毫秒。汇总值是各独立进程中位数的算术中位数；完整样本、每轮比例和内存指标见 comparison.json。超时不填作实际耗时。

| 场景 | 操作 | 旧版 | 优化版 | 降低比例 | 配对轮数 |
| --- | --- | ---: | ---: | ---: | ---: |
| cli-expanded-10k | project_inspect | 40.465 | 39.741 | 1.788% | 3 |
| cli-expanded-10k | schema_inspect | 45.996 | 42.024 | 8.636% | 3 |
| cli-expanded-10k | data_list | 408.916 | 411.159 | -0.548% | 3 |
| cli-expanded-10k | filter_sort | 422.377 | 426.644 | -1.010% | 3 |
| cli-expanded-10k | data_search | 414.681 | 406.206 | 2.044% | 3 |
| cli-expanded-10k | data_refs | 1267.911 | 1278.560 | -0.840% | 3 |
| cli-expanded-10k | check_structure | 386.875 | 382.512 | 1.128% | 3 |
| cli-expanded-10k | stats_field | 406.545 | 409.374 | -0.696% | 3 |
| cli-expanded-10k | dimension_get | 545.524 | 418.074 | 23.363% | 3 |
| cli-expanded-10k | dimension_missing | 661.732 | 524.026 | 20.810% | 3 |
| cli-expanded-10k | dimension_diff | 653.903 | 512.489 | 21.626% | 3 |
| cli-expanded-10k | dimension_query | 552.002 | 422.709 | 23.422% | 3 |
| cli-expanded-10k | set_where | 49614.530 | 28584.188 | 42.387% | 3 |
| cli-expanded-10k | array_insert | 22027.409 | 2301.225 | 89.553% | 3 |
| cli-expanded-10k | array_remove | 22103.391 | 2323.360 | 89.489% | 3 |
| cli-expanded-10k | array_move | 22123.097 | 2301.195 | 89.598% | 3 |
| cli-expanded-10k | dict_set | 22096.097 | 2299.665 | 89.592% | 3 |
| cli-expanded-10k | dict_remove | 22101.395 | 2323.531 | 89.487% | 3 |
| cli-expanded-10k | create_record | 21918.298 | 2327.744 | 89.380% | 3 |
| cli-expanded-10k | delete_record | 22077.432 | 2323.180 | 89.477% | 3 |
| cli-expanded-10k | rename_record | 22295.710 | 2503.654 | 88.771% | 3 |
| cli-expanded-10k | transfer_record | 22005.417 | 2327.688 | 89.422% | 3 |
| cli-expanded-10k | unset_field | 21890.484 | 2324.208 | 89.383% | 3 |
| cli-expanded-10k | dimension_set | 22054.862 | 2279.602 | 89.664% | 3 |
| cli-expanded-10k | dimension_add_variant | 36461.500 | 2829.467 | 92.240% | 3 |
| cli-expanded-10k | dimension_clear | 22226.819 | 2305.088 | 89.629% | 3 |
| cli-expanded-10k | source_write | 21973.828 | 2014.630 | 90.832% | 3 |
| cli-expanded-10k | reject_invalid_value | 11036.410 | 1242.567 | 88.741% | 3 |
| cli-expanded-10k | reject_invalid_source | 11119.085 | 1347.559 | 87.881% | 3 |
| cli-expanded-10k | repair_invalid_source | 11134.073 | 1382.099 | 87.587% | 3 |
| cli-expanded-10k | revision_conflict | 134.489 | 133.661 | 0.616% | 3 |
