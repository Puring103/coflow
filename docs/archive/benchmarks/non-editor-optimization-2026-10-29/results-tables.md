# 非编辑器性能最终结果表

本表使用最终冻结产物的正式证据：Rust／模型／CLI 为 R4，.NET／Unity Mono 为 R6。耗时单位为毫秒；`中位数 [最小–最大]` 的范围来自独立进程 P50，至少三对有效进程时采用同一组配对两侧统计。基线缺失时，优化侧可展示成功独立进程范围，相对收益保持空白。降低比例为 1 − 优化侧中位数 / 基线侧中位数；负值表示变慢。

状态直接引用 [目标验收](final-results/target-status.md)；5% 稳定性使用其独立条件。原始 [描述性比较](final-results/comparison.json) 中的部分失败运行不用于判定。超时、缺失和 Mono 不支持的分配计数均保留为空。MB 按十进制字节。查询批次、首次视图及零分配指标的单位在对应表中单列。

| 证据集合 | 状态 |
| --- | --- |
| 数值目标 | 达标 185，未达标 63，证据不足 7 |
| 基准进程峰值 | 达标 39，未达标 0，证据不足 3 |
| 独立 CLI 内存 | 达标 0，未达标 4，证据不足 0 |
| 最终计划与退出元数据 | 291 个计划，291 份已退出进程元数据 |

以上是逐项数值证据状态。G1–G13 的算法工作量、阶段时延、功能回归和 IL2CPP 宿主状态另见主报告与 [方法审查](verification/final-report-method-review.md)。`stability-r4` 补测独立报告，未混入本表。

## Rust Runtime 与模型

| 场景 / 操作 | 基线 P50 [范围] | 优化 P50 [范围] | 降低比例 | 有效配对 / 优化进程 | 独立条件 | 目标状态 |
| --- | --- | --- | --- | --- | --- | --- |
| 单文件 2 万条节点预算拒绝 | 21805.927 [21803.968–46591.038] | 453.856 [444.308–454.237] | 97.919% | 3 对 / 3 优化进程 | P50 ms ≤1000.000 ms | 达标 · [rust-node-budget](final-results/target-status.json) |
| 万条环形模板非法值 | — | 346.078 [339.404–347.328] | — | 0 对 / 3 优化进程 | P50 ms ≤2000.000 ms | 达标 · [rust-invalid-10k](final-results/target-status.json) |
| 十万条环形模板非法值 | — | 3794.199 [3791.790–6846.292] | — | 0 对 / 3 优化进程 | P50 ms ≤10000.000 ms | 达标 · [rust-invalid-100k](final-results/target-status.json) |
| 十万条健康加载（规则开启） | 6246.443 [4668.379–6316.792] | 6100.727 [4608.941–6118.758] | 2.333% | 3 对 / 3 优化进程 | P50 ms ≤6000.000 ms | 未达标 · [rust-load_checks](final-results/target-status.json) |
| 十万条健康加载（无规则） | 4222.598 [2487.917–4281.357] | 4130.166 [2476.519–4239.252] | 2.189% | 3 对 / 3 优化进程 | P50 ms ≤3000.000 ms | 未达标 · [rust-load_no_checks](final-results/target-status.json) |
| 十万条热标量修改 | 29.692 [29.223–49.312] | 29.916 [29.796–48.770] | -0.754% | 3 对 / 3 优化进程 | P50 ms ≤35.000 ms<br>观察 P95 ms ≤70.000 ms<br>P50 ms 优化/基线 ≤0.750 | 未达标 · [rust-hot-scalar](final-results/target-status.json) |
| 十万条同文件 1,000 项修改 | 124.715 [69.348–126.678] | 120.615 [67.792–127.608] | 3.288% | 3 对 / 3 优化进程 | P50 ms ≤100.000 ms<br>P50 ms 优化/基线 ≤0.750 | 未达标 · [rust-grouped-1000](final-results/target-status.json) |
| 十万条跨文件 1,000 项修改 | 2688.652 [2681.819–5097.205] | 4896.464 [2664.014–5056.392] | -82.116% | 3 对 / 3 优化进程 | P50 ms ≤1500.000 ms | 未达标 · [rust-interleaved-1000](final-results/target-status.json) |
| 万条十变体稀疏已有覆盖修改 | 33.650 [33.571–34.292] | 8.134 [8.051–8.425] | 75.827% | 3 对 / 3 优化进程 | P50 ms ≤30.000 ms | 达标 · [rust-sparse-dimension_existing_set](final-results/target-status.json) |
| 万条十变体稀疏已有覆盖清除 | 32.604 [32.089–59.214] | 8.194 [8.024–20.302] | 74.868% | 3 对 / 3 优化进程 | P50 ms ≤30.000 ms | 达标 · [rust-sparse-dimension_existing_clear](final-results/target-status.json) |
| 万条十变体密集已有覆盖修改 | 1900.424 [1889.088–2769.614] | 405.105 [398.470–421.569] | 78.683% | 3 对 / 3 优化进程 | P50 ms ≤500.000 ms | 达标 · [rust-dense-dimension_existing_set](final-results/target-status.json) |
| 万条十变体密集已有覆盖清除 | 1945.719 [1934.278–3535.471] | 436.313 [431.454–777.279] | 77.576% | 3 对 / 3 优化进程 | P50 ms ≤500.000 ms | 达标 · [rust-dense-dimension_existing_clear](final-results/target-status.json) |
| 万条稀疏一变体完整投影 | 58.547 [58.066–91.072] | 5.772 [5.753–6.744] | 90.141% | 3 对 / 3 优化进程 | P50 ms ≤30.000 ms | 达标 · [rust-projection](final-results/target-status.json) |
| 万条稀疏一变体相同选择 | 0.000500 [0.000300–0.000700] | 0.000500 [0.000300–0.000600] | 0% | 3 对 / 3 优化进程 | P50 ms 优化/基线 ≤1.050 | 达标 · [rust-projection-same](final-results/target-status.json) |
| 十万条含模板 insert_record | 3557.723 [3532.097–6435.704] | 1799.558 [1754.143–2979.518] | 49.418% | 3 对 / 3 优化进程 | P50 ms ≤500.000 ms | 未达标 · [rust-insert_record](final-results/target-status.json) |
| 十万条无模板 insert_record | 3371.626 [3293.768–5262.657] | 422.729 [411.838–647.352] | 87.462% | 3 对 / 3 优化进程 | P50 ms ≤500.000 ms | 达标 · [rust-literal-insert_record](final-results/target-status.json) |
| 十万条含模板 delete_record | 3410.709 [3376.418–6413.304] | 1741.381 [1724.596–3107.191] | 48.944% | 3 对 / 3 优化进程 | P50 ms ≤500.000 ms | 未达标 · [rust-delete_record](final-results/target-status.json) |
| 十万条无模板 delete_record | 3405.910 [3379.567–5990.757] | 376.867 [356.045–674.603] | 88.935% | 3 对 / 3 优化进程 | P50 ms ≤500.000 ms | 达标 · [rust-literal-delete_record](final-results/target-status.json) |
| 十万条含模板 rename_record | 3557.051 [3544.885–6534.314] | 1772.447 [1765.339–3151.835] | 50.171% | 3 对 / 3 优化进程 | P50 ms ≤500.000 ms | 未达标 · [rust-rename_record](final-results/target-status.json) |
| 十万条无模板 rename_record | 3349.313 [3328.191–6182.486] | 423.176 [401.090–723.570] | 87.365% | 3 对 / 3 优化进程 | P50 ms ≤500.000 ms | 达标 · [rust-literal-rename_record](final-results/target-status.json) |
| 十万条含模板 transfer_record | 3561.448 [3522.053–6501.109] | 1749.962 [1736.354–3095.442] | 50.864% | 3 对 / 3 优化进程 | P50 ms ≤500.000 ms | 未达标 · [rust-transfer_record](final-results/target-status.json) |
| 十万条无模板 transfer_record | 3380.797 [3349.234–6053.675] | 392.876 [368.636–666.172] | 88.379% | 3 对 / 3 优化进程 | P50 ms ≤500.000 ms | 达标 · [rust-literal-transfer_record](final-results/target-status.json) |

热标量观察 P95：基线 32.651 [31.748–57.614] ms，优化 32.775 [32.716–55.829] ms，达标；每个贡献进程至少 20 个原始样本。P95 范围描述各进程观察 P95，不能作为稳定尾延迟保证。

| 加载 / 恢复 | 独立 5% 稳定性 |
| --- | --- |
| 十万条含规则加载 | 达标（0.977；3 对） |
| 十万条无规则加载 | 达标（0.978；3 对） |
| 万条错误后恢复 | 证据不足（—；0 对） |

## CLI 基础 39 项操作

万条基础矩阵固定为 10 文件、20 字段、语言／平台各一变体，共登记 39 项操作；其中 39 项具有三对有效耗时证据。common 场景承载 8 项，expanded 场景承载其余 31 项。计划、退出元数据和验收成功分别计数，完成补采不自动表示达标。千条基准批次为 100 项，万条批次和 set_where 预算绑定实际 1,000 项。

| 场景 | 操作数 | 具备三对耗时 | 数值目标状态 |
| --- | --- | --- | --- |
| 原有核心操作 | 8 | 8 | 达标 4，未达标 4，证据不足 0 |
| 补充常见操作 | 31 | 31 | 达标 9，未达标 22，证据不足 0 |

| 万条操作 | 基线 P50 [范围] | 优化 P50 [范围] | 降低比例 | 有效配对 / 优化进程 | 目标状态 | 条件 5% 稳定性 |
| --- | --- | --- | --- | --- | --- | --- |
| project_inspect | 40.465 [39.110–46.361] | 39.741 [38.144–41.810] | 1.788% | 3 / 3 | 达标 · [cli-10000-project_inspect](final-results/target-status.json) | 达标（0.982；3 对） |
| schema_inspect | 45.996 [44.677–49.449] | 42.024 [41.778–43.143] | 8.636% | 3 / 3 | 达标 · [cli-10000-schema_inspect](final-results/target-status.json) | 达标（0.914；3 对） |
| data_get | 396.578 [392.236–399.254] | 396.483 [385.846–399.189] | 0.024% | 3 / 3 | 达标 · [cli-10000-data_get](final-results/target-status.json) | 达标（1.000；3 对） |
| data_list | 408.916 [406.615–410.118] | 411.159 [407.711–411.897] | -0.548% | 3 / 3 | 达标 · [cli-10000-data_list](final-results/target-status.json) | 达标（1.005；3 对） |
| filter_sort | 422.377 [421.957–423.935] | 426.644 [419.860–427.113] | -1.010% | 3 / 3 | 达标 · [cli-10000-filter_sort](final-results/target-status.json) | 达标（1.010；3 对） |
| data_search | 414.681 [409.939–415.570] | 406.206 [405.549–406.746] | 2.044% | 3 / 3 | 达标 · [cli-10000-data_search](final-results/target-status.json) | 达标（0.980；3 对） |
| data_refs | 1267.911 [1256.527–1292.804] | 1278.560 [1264.524–1314.483] | -0.840% | 3 / 3 | 未达标 · [cli-10000-data_refs](final-results/target-status.json) | 不适用（1.008；3 对） |
| check_structure | 386.875 [382.914–426.323] | 382.512 [379.595–388.051] | 1.128% | 3 / 3 | 达标 · [cli-10000-check_structure](final-results/target-status.json) | 达标（0.989；3 对） |
| check_rules | 471.051 [468.404–482.217] | 466.450 [464.328–478.747] | 0.977% | 3 / 3 | 达标 · [cli-10000-check_rules](final-results/target-status.json) | 达标（0.990；3 对） |
| stats_field | 406.545 [404.097–408.660] | 409.374 [404.792–412.926] | -0.696% | 3 / 3 | 达标 · [cli-10000-stats_field](final-results/target-status.json) | 达标（1.007；3 对） |
| dimension_get | 545.524 [543.722–546.612] | 418.074 [416.522–460.863] | 23.363% | 3 / 3 | 未达标 · [cli-10000-dimension_get](final-results/target-status.json) | 不适用（0.766；3 对） |
| dimension_missing | 661.732 [655.674–663.368] | 524.026 [521.072–524.284] | 20.810% | 3 / 3 | 未达标 · [cli-10000-dimension_missing](final-results/target-status.json) | 不适用（0.792；3 对） |
| dimension_diff | 653.903 [649.462–662.817] | 512.489 [509.044–520.557] | 21.626% | 3 / 3 | 未达标 · [cli-10000-dimension_diff](final-results/target-status.json) | 不适用（0.784；3 对） |
| dimension_query | 552.002 [551.587–581.356] | 422.709 [422.418–422.910] | 23.422% | 3 / 3 | 未达标 · [cli-10000-dimension_query](final-results/target-status.json) | 不适用（0.766；3 对） |
| set_field | 21809.533 [21717.276–21832.624] | 2218.258 [2217.962–2226.218] | 89.829% | 3 / 3 | 未达标 · [cli-10000-set_field](final-results/target-status.json) | 不适用（0.102；3 对） |
| set_field_rules | 21927.864 [21876.302–22430.669] | 2310.013 [2308.410–2335.956] | 89.465% | 3 / 3 | 未达标 · [cli-10000-set_field_rules](final-results/target-status.json) | 不适用（0.105；3 对） |
| set_noop | 21817.338 [21770.860–22250.662] | 2162.314 [2153.782–2165.738] | 90.089% | 3 / 3 | 未达标 · [cli-10k-set_noop](final-results/target-status.json) | 无该项数值判定 |
| set_preview | 21856.860 [21818.988–22258.134] | 2200.546 [2194.094–2232.961] | 89.932% | 3 / 3 | 未达标 · [cli-10k-set_preview](final-results/target-status.json) | 无该项数值判定 |
| batch_same_file | 49218.203 [49052.748–49697.077] | 2359.664 [2349.771–2446.098] | 95.206% | 3 / 3 | 达标 · [cli-10k-batch_same_file](final-results/target-status.json) | 无该项数值判定 |
| batch_cross_file | 48379.648 [48332.351–48775.463] | 2630.221 [2626.961–2667.412] | 94.563% | 3 / 3 | 达标 · [cli-10k-batch_cross_file](final-results/target-status.json) | 无该项数值判定 |
| set_where | 49614.530 [49432.725–49732.216] | 28584.188 [28478.396–29242.970] | 42.387% | 3 / 3 | 未达标 · [cli-10000-set_where](final-results/target-status.json) | 不适用（0.576；3 对） |
| array_insert | 22027.409 [22018.398–22738.257] | 2301.225 [2300.082–2315.501] | 89.553% | 3 / 3 | 未达标 · [cli-10000-array_insert](final-results/target-status.json) | 不适用（0.104；3 对） |
| array_remove | 22103.391 [22095.049–22219.769] | 2323.360 [2310.440–2382.748] | 89.489% | 3 / 3 | 未达标 · [cli-10000-array_remove](final-results/target-status.json) | 不适用（0.105；3 对） |
| array_move | 22123.097 [22025.129–22305.499] | 2301.195 [2289.829–2305.523] | 89.598% | 3 / 3 | 未达标 · [cli-10000-array_move](final-results/target-status.json) | 不适用（0.104；3 对） |
| dict_set | 22096.097 [22024.822–22135.562] | 2299.665 [2299.064–2348.437] | 89.592% | 3 / 3 | 未达标 · [cli-10000-dict_set](final-results/target-status.json) | 不适用（0.104；3 对） |
| dict_remove | 22101.395 [22039.531–22199.575] | 2323.531 [2317.009–2338.706] | 89.487% | 3 / 3 | 未达标 · [cli-10000-dict_remove](final-results/target-status.json) | 不适用（0.105；3 对） |
| create_record | 21918.298 [21843.952–21964.305] | 2327.744 [2280.534–2336.898] | 89.380% | 3 / 3 | 未达标 · [cli-10000-create_record](final-results/target-status.json) | 不适用（0.106；3 对） |
| delete_record | 22077.432 [22016.370–22140.931] | 2323.180 [2322.260–2326.000] | 89.477% | 3 / 3 | 未达标 · [cli-10000-delete_record](final-results/target-status.json) | 不适用（0.105；3 对） |
| rename_record | 22295.710 [22246.584–22535.968] | 2503.654 [2486.815–2552.119] | 88.771% | 3 / 3 | 未达标 · [cli-10000-rename_record](final-results/target-status.json) | 不适用（0.112；3 对） |
| transfer_record | 22005.417 [21788.593–22052.630] | 2327.688 [2325.462–2347.123] | 89.422% | 3 / 3 | 未达标 · [cli-10000-transfer_record](final-results/target-status.json) | 不适用（0.106；3 对） |
| unset_field | 21890.484 [21819.819–22086.762] | 2324.208 [2279.796–2351.485] | 89.383% | 3 / 3 | 未达标 · [cli-10000-unset_field](final-results/target-status.json) | 不适用（0.106；3 对） |
| dimension_set | 22054.862 [22041.502–22102.758] | 2279.602 [2266.593–2299.614] | 89.664% | 3 / 3 | 未达标 · [cli-10000-dimension_set](final-results/target-status.json) | 不适用（0.103；3 对） |
| dimension_add_variant | 36461.500 [35351.314–36491.843] | 2829.467 [2829.021–2875.575] | 92.240% | 3 / 3 | 达标 · [cli-10000-dimension_add_variant](final-results/target-status.json) | 不适用（0.078；3 对） |
| dimension_clear | 22226.819 [21989.434–22420.503] | 2305.088 [2263.886–2323.635] | 89.629% | 3 / 3 | 未达标 · [cli-10000-dimension_clear](final-results/target-status.json) | 不适用（0.104；3 对） |
| source_write | 21973.828 [21831.294–22335.209] | 2014.630 [1995.976–2019.670] | 90.832% | 3 / 3 | 未达标 · [cli-10000-source_write](final-results/target-status.json) | 不适用（0.092；3 对） |
| reject_invalid_value | 11036.410 [10921.648–11068.061] | 1242.567 [1234.953–1282.755] | 88.741% | 3 / 3 | 未达标 · [cli-10000-reject_invalid_value](final-results/target-status.json) | 不适用（0.113；3 对） |
| reject_invalid_source | 11119.085 [10890.046–11180.147] | 1347.559 [1316.609–1357.836] | 87.881% | 3 / 3 | 未达标 · [cli-10000-reject_invalid_source](final-results/target-status.json) | 不适用（0.121；3 对） |
| repair_invalid_source | 11134.073 [11080.676–11212.144] | 1382.099 [1374.507–1411.563] | 87.587% | 3 / 3 | 未达标 · [cli-10000-repair_invalid_source](final-results/target-status.json) | 不适用（0.124；3 对） |
| revision_conflict | 134.489 [131.073–140.673] | 133.661 [131.914–134.431] | 0.616% | 3 / 3 | 达标 · [cli-10000-revision_conflict](final-results/target-status.json) | 达标（0.994；3 对） |

同值、预览和安全批次沿用专项绝对预算；一般操作的 5% 条件仅在基线已满足相应绝对预算时适用。“不适用”不是稳定性通过。各操作的预算、影响数量、排除原因和每轮值见链接中的目标记录。

## .NET 与 Unity Mono 加载

| 宿主 / 夹具 | 基线 P50 [范围] | 优化 P50 [范围] | 降低比例 | 有效配对 | 独立 5% 稳定性 | 专项目标 |
| --- | --- | --- | --- | --- | --- | --- |
| .NET / scalar20-1k | 35.430 [34.881–36.933] | 19.383 [18.949–19.696] | 45.293% | 3 | 达标（0.547） | 稳定性 · [stability-csharp-scalar20-1k:load_text](final-results/target-status.json) |
| .NET / scalar20-100k | 2987.610 [2964.174–2998.172] | 1572.141 [1540.862–1589.365] | 47.378% | 3 | 达标（0.526） | 达标 · [dotnet-scalar100k](final-results/target-status.json) |
| .NET / width8-10k | 253.298 [218.959–274.047] | 120.940 [119.764–131.154] | 52.254% | 3 | 达标（0.477） | 稳定性 · [stability-csharp-width8-10k:load_text](final-results/target-status.json) |
| .NET / width100-10k | 1505.783 [1434.204–1517.658] | 653.915 [617.786–665.132] | 56.573% | 3 | 达标（0.434） | 达标 · [dotnet-width100](final-results/target-status.json) |
| .NET / tables32-10k | 332.944 [328.817–340.521] | 192.093 [189.671–262.285] | 42.305% | 3 | 达标（0.577） | 稳定性 · [stability-csharp-tables32-10k:load_text](final-results/target-status.json) |
| .NET / complex-100k | 4395.396 [4274.213–4432.910] | 2647.143 [2615.304–2656.146] | 39.775% | 3 | 达标（0.602） | 达标 · [dotnet-complex100k](final-results/target-status.json) |
| .NET / templates-10k | 474.840 [469.520–503.689] | 341.184 [277.041–345.817] | 28.148% | 3 | 达标（0.719） | 稳定性 · [stability-csharp-templates-10k:load_text](final-results/target-status.json) |
| .NET / ring-10k | 474.033 [468.815–474.830] | 316.043 [312.335–343.923] | 33.329% | 3 | 达标（0.667） | 稳定性 · [stability-csharp-ring-10k:load_text](final-results/target-status.json) |
| .NET / dim1-sparse-10k | 896.898 [852.863–897.285] | 389.973 [373.717–406.894] | 56.520% | 3 | 达标（0.435） | 未达标 · [csharp-dim1-sparse-10k](final-results/target-status.json) |
| .NET / dim10-sparse-10k | 4311.501 [4247.790–4333.593] | 1386.229 [1376.119–1433.660] | 67.848% | 3 | 达标（0.322） | 达标 · [csharp-dim10-sparse-10k](final-results/target-status.json) |
| .NET / dim10-dense-10k | 4935.590 [4929.688–5019.424] | 1706.713 [1639.762–1745.236] | 65.420% | 3 | 达标（0.346） | 稳定性 · [stability-csharp-dim10-dense-10k:load_text](final-results/target-status.json) |
| .NET / dim10-no-templates-10k | 4078.983 [4039.344–4083.140] | 1200.715 [1170.125–1213.218] | 70.563% | 3 | 达标（0.294） | 达标 · [csharp-dim10-no-templates-10k](final-results/target-status.json) |
| Mono / scalar20-1k | 40.474 [39.935–41.086] | 20.932 [20.508–23.268] | 48.283% | 3 | 达标（0.517） | 稳定性 · [stability-mono-scalar20-1k:load_text](final-results/target-status.json) |
| Mono / scalar20-100k | 5744.815 [5743.602–5775.993] | 2583.549 [2544.849–2630.494] | 55.028% | 3 | 达标（0.450） | 达标 · [mono-scalar20-100k](final-results/target-status.json) |
| Mono / width8-10k | 187.331 [187.053–188.826] | 108.403 [103.720–108.876] | 42.133% | 3 | 达标（0.579） | 稳定性 · [stability-mono-width8-10k:load_text](final-results/target-status.json) |
| Mono / width100-10k | 3418.815 [3379.160–3421.267] | 1093.165 [1091.601–1094.351] | 68.025% | 3 | 达标（0.320） | 稳定性 · [stability-mono-width100-10k:load_text](final-results/target-status.json) |
| Mono / tables32-10k | 478.944 [478.032–487.875] | 233.905 [232.731–238.989] | 51.162% | 3 | 达标（0.488） | 稳定性 · [stability-mono-tables32-10k:load_text](final-results/target-status.json) |
| Mono / complex-100k | 7304.704 [7239.007–7334.010] | 3732.940 [3721.036–3790.133] | 48.897% | 3 | 达标（0.511） | 达标 · [mono-complex-100k](final-results/target-status.json) |
| Mono / templates-10k | 668.915 [659.443–676.426] | 398.555 [395.512–400.215] | 40.418% | 3 | 达标（0.596） | 稳定性 · [stability-mono-templates-10k:load_text](final-results/target-status.json) |
| Mono / ring-10k | 677.142 [666.906–679.760] | 391.890 [391.235–424.123] | 42.126% | 3 | 达标（0.579） | 稳定性 · [stability-mono-ring-10k:load_text](final-results/target-status.json) |
| Mono / dim1-sparse-10k | 1286.138 [1270.477–1386.257] | 535.505 [533.220–541.056] | 58.363% | 3 | 达标（0.416） | 未达标 · [mono-dim1-sparse-10k](final-results/target-status.json) |
| Mono / dim10-sparse-10k | 7666.708 [7657.441–7735.893] | 1698.353 [1695.248–1711.862] | 77.848% | 3 | 达标（0.222） | 证据不足 · [mono-dim10-sparse-10k](final-results/target-status.json) |
| Mono / dim10-dense-10k | 7968.768 [7930.744–8021.548] | 2192.149 [2181.408–2254.806] | 72.491% | 3 | 达标（0.275） | 稳定性 · [stability-mono-dim10-dense-10k:load_text](final-results/target-status.json) |
| Mono / dim10-no-templates-10k | 6791.120 [6788.839–6911.608] | 1283.755 [1265.440–1300.781] | 81.097% | 3 | 达标（0.189） | 证据不足 · [mono-dim10-no-templates-10k](final-results/target-status.json) |

标量十万条、宽表、复杂数据、维度加载的专项目标与 5% 回归条件分别判定。Mono 的十万条标量／复杂加载同时要求绝对预算和相对本宿主基线至少 40% 改善。

## 托管分配、保留内存与维度

| 场景 | 指标 | 基线 MB [范围] | 优化 MB [范围] | 条件 | 判定 / 配对 | 证据 |
| --- | --- | --- | --- | --- | --- | --- |
| dotnet-scalar100k | 分配 MB | 3079.748 [3079.748–3079.748] | 1217.398 [1217.398–1217.398] | 分配 MB ≤1500.000 MB | 达标；3 对 | [dotnet-scalar100k](final-results/target-status.json) |
| dotnet-scalar100k | GC 后保留增量 MB | 814.775 [814.766–814.775] | 318.556 [318.556–318.564] | GC 后保留增量 MB ≤400.000 MB | 达标；3 对 | [dotnet-scalar100k](final-results/target-status.json) |
| dotnet-width100 | 分配 MB | 1452.464 [1452.464–1452.464] | 552.256 [552.256–552.258] | 分配 MB ≤750.000 MB | 达标；3 对 | [dotnet-width100](final-results/target-status.json) |
| dotnet-complex100k | 分配 MB | 3810.958 [3810.958–3810.960] | 1608.624 [1608.624–1608.624] | 分配 MB ≤1900.000 MB | 达标；3 对 | [dotnet-complex100k](final-results/target-status.json) |
| dotnet-complex100k | GC 后保留增量 MB | 1174.732 [1174.732–1174.740] | 532.755 [532.755–532.764] | GC 后保留增量 MB ≤600.000 MB | 达标；3 对 | [dotnet-complex100k](final-results/target-status.json) |
| dotnet-first-view | 分配 MB | 171.516 [171.515–171.516] | 22.795 [22.795–22.796] | 分配 MB 优化/基线 ≤0.500 | 达标；3 对 | [dotnet-first-view](final-results/target-status.json) |
| .NET / dim1-sparse-10k | 分配 MB | 731.447 [731.446–731.447] | 232.355 [232.330–232.355] | 分配 MB 优化/基线 ≤0.400 | 达标；3 对 | [csharp-dim1-sparse-10k](final-results/target-status.json) |
| .NET / dim10-sparse-10k | 分配 MB | 3664.271 [3664.270–3664.273] | 619.594 [619.594–619.594] | 分配 MB 优化/基线 ≤0.400 | 达标；3 对 | [csharp-dim10-sparse-10k](final-results/target-status.json) |
| .NET / dim10-no-templates-10k | 分配 MB | 3319.519 [3319.519–3319.525] | 501.642 [501.642–501.645] | 分配 MB 优化/基线 ≤0.400 | 达标；3 对 | [csharp-dim10-no-templates-10k](final-results/target-status.json) |
| Mono / dim1-sparse-10k | 当前线程分配 MB | — | — | 当前线程分配 MB 优化/基线 ≤0.400 | 证据不足；0 对 | [mono-dim1-sparse-10k](final-results/target-status.json) |
| Mono / dim10-sparse-10k | 当前线程分配 MB | — | — | 当前线程分配 MB 优化/基线 ≤0.400 | 证据不足；0 对 | [mono-dim10-sparse-10k](final-results/target-status.json) |
| Mono / dim10-no-templates-10k | 当前线程分配 MB | — | — | 当前线程分配 MB 优化/基线 ≤0.400 | 证据不足；0 对 | [mono-dim10-no-templates-10k](final-results/target-status.json) |

| 首次视图 | 基线 ms [范围] | 优化 ms [范围] | 降低比例 | 有效配对 / 优化进程 | 独立条件 | 目标状态 |
| --- | --- | --- | --- | --- | --- | --- |
| .NET 万条首次全表视图 | 165.617 [165.590–168.212] | 29.646 [29.103–29.778] | 82.099% | 3 对 / 3 优化进程 | P50 ms ≤150.000 ms<br>分配 MB 优化/基线 ≤0.500 | 达标 · [dotnet-first-view](final-results/target-status.json) |

分配表示被测操作的托管分配量；保留增量是 GC 后观察值。Mono 空分配计数不参与数值比较，也不等于零分配。维度加载相对收益和全表首次视图分配收益均使用本宿主、同夹具的有效配对。

## 托管查询与缓存维度

| 宿主 / 夹具 | 查询 | 每样本操作数 | 基线批次 ms [范围] | 优化批次 ms [范围] | 有效配对 | 5% 稳定性 |
| --- | --- | --- | --- | --- | --- | --- |
| .NET / scalar20-100k | table_lookup_batch | 10000 | 0.582 [0.575–0.611] | 0.499 [0.494–0.503] | 3 | 达标（0.856） · [stability-csharp-scalar20-100k:table_lookup_batch](final-results/target-status.json) |
| .NET / scalar20-100k | direct_property_batch | 10000 | 0.020 [0.019–0.021] | 0.021 [0.019–0.021] | 3 | 未达标（1.054） · [stability-csharp-scalar20-100k:direct_property_batch](final-results/target-status.json) |
| .NET / scalar20-100k | table_enumerate | 100000 | 0.834 [0.827–0.877] | 0.744 [0.716–0.745] | 3 | 达标（0.891） · [stability-csharp-scalar20-100k:table_enumerate](final-results/target-status.json) |
| .NET / width100-10k | table_lookup_batch | 10000 | 0.406 [0.399–0.420] | 0.362 [0.344–0.372] | 3 | 达标（0.893） · [stability-csharp-width100-10k:table_lookup_batch](final-results/target-status.json) |
| .NET / width100-10k | direct_property_batch | 10000 | 0.030 [0.019–0.037] | 0.019 [0.019–0.039] | 3 | 达标（0.635） · [stability-csharp-width100-10k:direct_property_batch](final-results/target-status.json) |
| .NET / width100-10k | table_enumerate | 10000 | 0.091 [0.091–0.093] | 0.092 [0.090–0.095] | 3 | 达标（1.013） · [stability-csharp-width100-10k:table_enumerate](final-results/target-status.json) |
| .NET / tables32-10k | table_lookup_batch | 10000 | 0.339 [0.321–0.340] | 0.298 [0.291–0.312] | 3 | 达标（0.878） · [stability-csharp-tables32-10k:table_lookup_batch](final-results/target-status.json) |
| .NET / tables32-10k | direct_property_batch | 10000 | 0.025 [0.023–0.026] | 0.028 [0.025–0.028] | 3 | 未达标（1.098） · [stability-csharp-tables32-10k:direct_property_batch](final-results/target-status.json) |
| .NET / tables32-10k | table_enumerate | 10000 | 0.089 [0.087–0.092] | 0.081 [0.079–0.081] | 3 | 达标（0.908） · [stability-csharp-tables32-10k:table_enumerate](final-results/target-status.json) |
| .NET / ring-10k | table_lookup_batch | 10000 | 0.458 [0.424–0.461] | 0.370 [0.366–0.372] | 3 | 达标（0.809） · [stability-csharp-ring-10k:table_lookup_batch](final-results/target-status.json) |
| .NET / ring-10k | direct_property_batch | 10000 | 0.021 [0.020–0.021] | 0.021 [0.020–0.033] | 3 | 达标（0.998） · [stability-csharp-ring-10k:direct_property_batch](final-results/target-status.json) |
| .NET / ring-10k | table_enumerate | 10000 | 0.111 [0.110–0.113] | 0.107 [0.106–0.113] | 3 | 达标（0.971） · [stability-csharp-ring-10k:table_enumerate](final-results/target-status.json) |
| Mono / scalar20-100k | table_lookup_batch | 10000 | 0.477 [0.455–0.478] | 0.398 [0.388–0.399] | 3 | 达标（0.835） · [stability-mono-scalar20-100k:table_lookup_batch](final-results/target-status.json) |
| Mono / scalar20-100k | direct_property_batch | 10000 | 0.049 [0.047–0.053] | 0.047 [0.047–0.047] | 3 | 达标（0.959） · [stability-mono-scalar20-100k:direct_property_batch](final-results/target-status.json) |
| Mono / scalar20-100k | table_enumerate | 100000 | 0.754 [0.742–0.800] | 0.772 [0.765–0.779] | 3 | 达标（1.024） · [stability-mono-scalar20-100k:table_enumerate](final-results/target-status.json) |
| Mono / width100-10k | table_lookup_batch | 10000 | 0.397 [0.384–0.463] | 0.353 [0.353–0.361] | 3 | 达标（0.891） · [stability-mono-width100-10k:table_lookup_batch](final-results/target-status.json) |
| Mono / width100-10k | direct_property_batch | 10000 | 0.051 [0.051–0.051] | 0.045 [0.045–0.045] | 3 | 达标（0.885） · [stability-mono-width100-10k:direct_property_batch](final-results/target-status.json) |
| Mono / width100-10k | table_enumerate | 10000 | 0.071 [0.071–0.071] | 0.069 [0.069–0.069] | 3 | 达标（0.972） · [stability-mono-width100-10k:table_enumerate](final-results/target-status.json) |
| Mono / tables32-10k | table_lookup_batch | 10000 | 0.804 [0.757–0.805] | 0.709 [0.704–0.712] | 3 | 达标（0.882） · [stability-mono-tables32-10k:table_lookup_batch](final-results/target-status.json) |
| Mono / tables32-10k | direct_property_batch | 10000 | 0.045 [0.045–0.047] | 0.047 [0.047–0.047] | 3 | 达标（1.044） · [stability-mono-tables32-10k:direct_property_batch](final-results/target-status.json) |
| Mono / tables32-10k | table_enumerate | 10000 | 0.068 [0.067–0.071] | 0.069 [0.069–0.071] | 3 | 达标（1.025） · [stability-mono-tables32-10k:table_enumerate](final-results/target-status.json) |
| Mono / ring-10k | table_lookup_batch | 10000 | 0.364 [0.364–0.366] | 0.374 [0.328–0.381] | 3 | 达标（1.028） · [stability-mono-ring-10k:table_lookup_batch](final-results/target-status.json) |
| Mono / ring-10k | direct_property_batch | 10000 | 0.047 [0.047–0.047] | 0.047 [0.047–0.047] | 3 | 达标（1.000） · [stability-mono-ring-10k:direct_property_batch](final-results/target-status.json) |
| Mono / ring-10k | table_enumerate | 10000 | 0.071 [0.071–0.072] | 0.067 [0.067–0.068] | 3 | 达标（0.943） · [stability-mono-ring-10k:table_enumerate](final-results/target-status.json) |

| 宿主 | 缓存操作 | 每样本调用数 | 优化分配最大值 | 有效分配进程 | 零分配判定 | 时延预算 | 目标状态 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| .NET | dimension_cached_for_batch | 10000 | 0 B/批次 | 3 | 达标 | 0.495 [0.476–0.529] ms；达标 | 达标 · [csharp-dimension_cached_for_batch](final-results/target-status.json) |
| .NET | dimension_variants_batch | 10000 | 0 B/批次 | 3 | 达标 | 无独立时延预算 | 达标 · [csharp-dimension_variants_batch](final-results/target-status.json) |
| .NET | dimension_has_variant_batch | 10000 | 0 B/批次 | 3 | 达标 | 无独立时延预算 | 达标 · [csharp-dimension_has_variant_batch](final-results/target-status.json) |
| Mono | dimension_cached_for_batch | 10000 | —（计数不可用） | 0 | 证据不足 | 无独立时延预算 | 证据不足 · [mono-dimension_cached_for_batch](final-results/target-status.json) |
| Mono | dimension_variants_batch | 10000 | —（计数不可用） | 0 | 证据不足 | 无独立时延预算 | 证据不足 · [mono-dimension_variants_batch](final-results/target-status.json) |
| Mono | dimension_has_variant_batch | 10000 | —（计数不可用） | 0 | 证据不足 | 无独立时延预算 | 证据不足 · [mono-dimension_has_variant_batch](final-results/target-status.json) |

缓存零分配条件使用所有贡献进程全部原始样本的最大值，每个样本为一万次调用；该条件比仅观察中位数为零更严格。查询计时仍按完整批次展示。

## 独立 CLI 进程内存

直接采样 24/24 个命令进程，其中有效 24；夹具复核通过 12/12。每项采用 AB/BA/AB 三对进程；基线与优化范围仅来自有效配对，排除样本保留在 [独立内存报告](cli-memory-r4/cli-memory-report.md)。

| 命令 / 规模 | 基线峰值 MB [范围] | 优化峰值 MB [范围] | 峰值比 | 有效配对 | 绝对预算 MB | 绝对 / 5% / 总状态 |
| --- | --- | --- | --- | --- | --- | --- |
| set / 10000 | 1200.218 [1200.050–1200.259] | 744.198 [744.182–744.210] | 0.620 | 3 | 500.000 | 未达标 / 达标 / 未达标 |
| batch / 10000 | 1203.716 [1203.626–1207.095] | 770.417 [769.614–771.297] | 0.640 | 3 | 500.000 | 未达标 / 达标 / 未达标 |
| get / 100000 | 2277.274 [2277.274–2277.327] | 2280.759 [2280.579–2280.776] | 1.002 | 3 | 1200.000 | 未达标 / 达标 / 未达标 |
| check / 100000 | 2277.233 [2276.950–2277.253] | 2280.681 [2280.645–2280.731] | 1.002 | 3 | 1200.000 | 未达标 / 达标 / 未达标 |

该峰值属于单个 coflow.exe 命令，包含命令内加载、修改及输出；外部 Node／PowerShell、夹具生成及复核不计入。OS 累计工作集峰值约每 100 ms 观察，最后区间可能缺测；此表不推导分配量、常驻堆或时延分布。

## 未达标与证据不足

**未达标的数值条件**

| 目标 | 条件 | 验收观测值 | 有效证据 | 原因 |
| --- | --- | --- | --- | --- |
| [rust-load_checks](final-results/target-status.json) | P50 ms ≤6000.000 ms | 6100.727 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [rust-load_no_checks](final-results/target-status.json) | P50 ms ≤3000.000 ms | 4130.166 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [rust-hot-scalar](final-results/target-status.json) | P50 ms 优化/基线 ≤0.750 | 1.008 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [rust-grouped-1000](final-results/target-status.json) | P50 ms ≤100.000 ms | 120.615 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [rust-grouped-1000](final-results/target-status.json) | P50 ms 优化/基线 ≤0.750 | 0.967 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [rust-interleaved-1000](final-results/target-status.json) | P50 ms ≤1500.000 ms | 4896.464 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [rust-insert_record](final-results/target-status.json) | P50 ms ≤500.000 ms | 1799.558 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [rust-delete_record](final-results/target-status.json) | P50 ms ≤500.000 ms | 1741.381 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [rust-rename_record](final-results/target-status.json) | P50 ms ≤500.000 ms | 1772.447 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [rust-transfer_record](final-results/target-status.json) | P50 ms ≤500.000 ms | 1749.962 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-data_refs](final-results/target-status.json) | P50 ms ≤750.000 ms | 1278.560 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-dimension_get](final-results/target-status.json) | P50 ms ≤400.000 ms | 418.074 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-dimension_missing](final-results/target-status.json) | P50 ms ≤400.000 ms | 524.026 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-dimension_diff](final-results/target-status.json) | P50 ms ≤400.000 ms | 512.489 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-dimension_query](final-results/target-status.json) | P50 ms ≤400.000 ms | 422.709 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-set_field](final-results/target-status.json) | P50 ms ≤200.000 ms | 370.972 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-set_field_rules](final-results/target-status.json) | P50 ms ≤200.000 ms | 377.676 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-array_insert](final-results/target-status.json) | P50 ms ≤200.000 ms | 373.336 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-array_remove](final-results/target-status.json) | P50 ms ≤200.000 ms | 381.094 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-array_move](final-results/target-status.json) | P50 ms ≤200.000 ms | 372.369 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-dict_set](final-results/target-status.json) | P50 ms ≤200.000 ms | 374.218 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-dict_remove](final-results/target-status.json) | P50 ms ≤200.000 ms | 370.042 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-create_record](final-results/target-status.json) | P50 ms ≤200.000 ms | 390.384 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-delete_record](final-results/target-status.json) | P50 ms ≤200.000 ms | 377.653 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-rename_record](final-results/target-status.json) | P50 ms ≤200.000 ms | 428.287 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-transfer_record](final-results/target-status.json) | P50 ms ≤200.000 ms | 382.512 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-unset_field](final-results/target-status.json) | P50 ms ≤200.000 ms | 373.212 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-dimension_set](final-results/target-status.json) | P50 ms ≤200.000 ms | 356.854 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-dimension_clear](final-results/target-status.json) | P50 ms ≤200.000 ms | 370.224 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-source_write](final-results/target-status.json) | P50 ms ≤200.000 ms | 329.125 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-set_field](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2218.258 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-set_field_rules](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2310.013 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-array_insert](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2301.225 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-array_remove](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2323.360 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-array_move](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2301.195 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-dict_set](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2299.665 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-dict_remove](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2323.531 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-create_record](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2327.744 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-delete_record](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2323.180 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-rename_record](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2503.654 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-transfer_record](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2327.688 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-unset_field](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2324.208 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-dimension_set](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2279.602 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-dimension_clear](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2305.088 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-source_write](final-results/target-status.json) | P50 ms ≤2000.000 ms | 2014.630 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-dimension_add_variant](final-results/target-status.json) | P50 ms ≤300.000 ms | 424.490 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-set_where](final-results/target-status.json) | P50 ms ≤500.000 ms | 1493.700 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-set_where](final-results/target-status.json) | P50 ms ≤5000.000 ms | 28584.188 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-reject_invalid_value](final-results/target-status.json) | P50 ms ≤200.000 ms | 244.300 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-reject_invalid_source](final-results/target-status.json) | P50 ms ≤200.000 ms | 265.079 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-repair_invalid_source](final-results/target-status.json) | P50 ms ≤200.000 ms | 299.670 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-reject_invalid_value](final-results/target-status.json) | P50 ms ≤1000.000 ms | 1242.567 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-reject_invalid_source](final-results/target-status.json) | P50 ms ≤1000.000 ms | 1347.559 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10000-repair_invalid_source](final-results/target-status.json) | P50 ms ≤1000.000 ms | 1382.099 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-1000-revision_conflict](final-results/target-status.json) | P50 ms 优化/基线 ≤1.050（基线 ≤150.000 ms 时） | 1.144 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10k-set_noop](final-results/target-status.json) | P50 ms ≤1000.000 ms | 2162.314 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [cli-10k-set_preview](final-results/target-status.json) | P50 ms ≤1000.000 ms | 2200.546 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [csharp-dim1-sparse-10k](final-results/target-status.json) | P50 ms 优化/基线 ≤0.400 | 0.435 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [mono-dim1-sparse-10k](final-results/target-status.json) | P50 ms 优化/基线 ≤0.400 | 0.416 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [stability-csharp-scalar20-100k:direct_property_batch](final-results/target-status.json) | P50 ms 优化/基线 ≤1.050 | 1.054 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [stability-csharp-width8-10k:direct_property_batch](final-results/target-status.json) | P50 ms 优化/基线 ≤1.050 | 1.078 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [stability-csharp-tables32-10k:direct_property_batch](final-results/target-status.json) | P50 ms 优化/基线 ≤1.050 | 1.098 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [stability-mono-dim1-sparse-10k:reject_unknown_type_at_end](final-results/target-status.json) | P50 ms 优化/基线 ≤1.050 | 1.159 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |
| [stability-mono-dim1-sparse-10k:reject_syntax_at_end](final-results/target-status.json) | P50 ms 优化/基线 ≤1.050 | 1.166 | 3 对 / 3 优化进程 | 有效证据超过该条件阈值 |

**证据不足的数值条件**

| 目标 | 条件 | 验收观测值 | 有效证据 | 原因 |
| --- | --- | --- | --- | --- |
| [mono-dim1-sparse-10k](final-results/target-status.json) | 当前线程分配 MB 优化/基线 ≤0.400 | — | 0 对 / 0 优化进程 | 有效优化进程 0/3；有效配对 0/3；分配／内存计数缺测或不支持 |
| [mono-dim10-sparse-10k](final-results/target-status.json) | 当前线程分配 MB 优化/基线 ≤0.400 | — | 0 对 / 0 优化进程 | 有效优化进程 0/3；有效配对 0/3；分配／内存计数缺测或不支持 |
| [mono-dim10-no-templates-10k](final-results/target-status.json) | 当前线程分配 MB 优化/基线 ≤0.400 | — | 0 对 / 0 优化进程 | 有效优化进程 0/3；有效配对 0/3；分配／内存计数缺测或不支持 |
| [mono-dimension_cached_for_batch](final-results/target-status.json) | 当前线程分配 MB ≤0 MB | — | 0 对 / 0 优化进程 | 有效优化进程 0/3；分配／内存计数缺测或不支持 |
| [mono-dimension_variants_batch](final-results/target-status.json) | 当前线程分配 MB ≤0 MB | — | 0 对 / 0 优化进程 | 有效优化进程 0/3；分配／内存计数缺测或不支持 |
| [mono-dimension_has_variant_batch](final-results/target-status.json) | 当前线程分配 MB ≤0 MB | — | 0 对 / 0 优化进程 | 有效优化进程 0/3；分配／内存计数缺测或不支持 |
| [il2cpp-real-player](final-results/target-status.json) | P50 ms ≤8000.000 ms | — | 0 对 / 0 优化进程 | 缺少真实 IL2CPP Player 证据；有效优化进程 0/3 |
| [rust-recovery](final-results/target-status.json) | P50 ms 优化/基线 ≤1.050 | — | 0 对 / 3 优化进程 | 有效配对 0/3；失败／截尾进程 3 |

**未通过的进程峰值条件**

| 场景 | 基线峰值 MB [范围] | 优化峰值 MB [范围] | 峰值比 | 有效配对 | 状态 / 原因 |
| --- | --- | --- | --- | --- | --- |
| rust-invalid-10000 | — | 355.787（范围缺测） | — | 0 | 证据不足；有效配对 0/3；失败／截尾进程 3 |
| rust-invalid-100000 | — | 3215.237（范围缺测） | — | 0 | 证据不足；有效配对 0/3 |
| rust-recovery-10k | — | 355.946（范围缺测） | — | 0 | 证据不足；有效配对 0/3；失败／截尾进程 3 |

最终证据保留 9 个失败或截尾进程，CLI 操作失败记录 9 条。进程失败与操作失败分别计数；时限覆盖整个进程，未折算成操作毫秒或收益倍率。

| 进程 | 退出状态 | 时限口径 |
| --- | --- | --- |
| cli-main-100k-r1-baseline | 失败，退出码 1 | — |
| cli-main-100k-r2-baseline | 失败，退出码 1 | — |
| cli-main-100k-r3-baseline | 失败，退出码 1 | — |
| rust-invalid-10000-r1-baseline | 超时截尾 | 整进程上限 120 秒 |
| rust-invalid-10000-r2-baseline | 超时截尾 | 整进程上限 120 秒 |
| rust-invalid-10000-r3-baseline | 超时截尾 | 整进程上限 120 秒 |
| rust-recovery-10k-r1-baseline | 超时截尾 | 整进程上限 120 秒 |
| rust-recovery-10k-r2-baseline | 超时截尾 | 整进程上限 120 秒 |
| rust-recovery-10k-r3-baseline | 超时截尾 | 整进程上限 120 秒 |

相对收益、绝对预算、功能能力及宿主完整性分别验收。剩余优化与补证据计划参见主报告；本表中的数量不表示所有算法目标已经实现。

## 输入与复核

| 输入 | SHA256 |
| --- | --- |
| [summary](final-results/summary.json) | 8446c3fe8cab482e153bbf01daca24e3b8a28138ddf9344cd96b9dc6f94f15fd |
| [comparison](final-results/comparison.json) | b4251f9e36ac997f1185f7eb85c1845d3449b12bdbc7a32b54b596f102de5d6e |
| [targets](final-results/target-status.json) | c9116281554ca169c66e835fbf74228ee9828934d325bf3617fbd819847c48d1 |
| [memory](cli-memory-r4/cli-memory-report.json) | 3fa48f62e4865cf4b63cb7200f4e368081d41efeca3167e433f71a11ab42df9c |
| [provenance](final-results/evidence-provenance.json) | 7f8bcf6440ede16273c617e6cad15e224bc9efa2f4ce4a10a8cd08eb3dee6816 |

本表只展示目标报告已给出的条件状态；目标报告与独立证据审计需联合阅读。三轮原值、夹具、影响数量、排除原因、原始输出及来源摘要由最终证据目录保留。

