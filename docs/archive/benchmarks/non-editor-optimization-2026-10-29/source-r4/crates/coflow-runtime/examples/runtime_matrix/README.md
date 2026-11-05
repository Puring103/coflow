# Runtime 常见操作性能矩阵

工具仅使用公共核心 Runtime API，独立创建和清理临时工程。运行前构建 release 程序，测量期间停止其他构建和性能采样。

```powershell
cargo build --release -p coflow-runtime --example runtime_matrix
$env:MATRIX_RECORDS = '1000'
$env:MATRIX_FILES = '10'
$env:MATRIX_FIELDS = '20'
$env:MATRIX_SAMPLES = '3'
$env:MATRIX_CASES = 'load_checks,load_no_checks,set_scalar,batch_grouped'
target/release/examples/runtime_matrix.exe > matrix.ndjson
```

## 参数

| 环境变量 | 默认值 | 含义 |
| --- | ---: | --- |
| `MATRIX_RECORDS` | 1000 | 基础记录数量，至少 2，须被文件数整除 |
| `MATRIX_FILES` | 10 | 基础 CFD 文件数量 |
| `MATRIX_FIELDS` | 20 | 字段数量，至少 8；含数值、名称、模板、引用、数组、字典、Option、内联对象 |
| `MATRIX_SAMPLES` | 3 | 保留样本数量 |
| `MATRIX_WARMUPS` | 1 | 不进入统计的预热次数 |
| `MATRIX_BATCH` | 100 | 一次字段事务中的修改记录数，不超过总记录数 |
| `MATRIX_COLLECTION` | 8 | 每条基础记录的数组长度；集合替换写入长度 |
| `MATRIX_VARIANTS` | 0 | language 和 platform 各自的已声明变体数 |
| `MATRIX_STRIDE` | 100 | 每个变体每隔多少条记录提供覆盖；1 为密集覆盖 |
| `MATRIX_TOPOLOGY` | ring | `ring` 每条引用下一条；`star` 全部指向第零条 |
| `MATRIX_TEMPLATES` | 1 | 1 为 caption 读取 parent.name；0 为字面量 |
| `MATRIX_QUERY_BATCH` | 1000 | 查询类每个计时样本执行的查询数 |
| `MATRIX_CASES` | 全部 32 个常规操作 | 逗号分隔操作名；预算拒绝和已有维度文件修改场景需显式选择 |

基础 name 绑定 language，caption 绑定 platform。维度输入规模是两维各 V 个变体，每变体约 N/stride 条覆盖；不是 V² 个组合视图。基准的标量热修改目标为 name，依赖修改目标为 value，分别覆盖模板读者传播与检查读者传播。

## 操作与边界

| 操作 | 测量内容 |
| --- | --- |
| `load_checks` / `load_no_checks` | 新会话加载，分别执行/不执行业务检查；包括 schema 编译与数据加载，不包括夹具生成 |
| `load_budget_rejected` | 显式选择的单文件语法节点预算拒绝；不执行业务检查，断言具体超限诊断，记录保留记录数与错误数 |
| `field_lookup` | 批量记录字段查找 |
| `record_scan` | 遍历全部基础记录及字段入口，operations_per_sample 为记录数 |
| `references` | 批量查询记录的直接出边 |
| `validate_same` / `validate_changed` / `validate_invalid` | 同内容校验、变化文本候选校验、错误类型诊断 |
| `refresh_same` | 相同内容文件刷新 |
| `error_recovery` | 从已加载错误状态恢复健康磁盘内容，错误状态准备不计时 |
| `set_scalar` / `set_dependency` / `set_noop` | 修改名称、修改被规则读取的数值、相同值写入 |
| `unset_field` | 移除显式 value 并恢复声明默认值 |
| `batch_grouped` / `batch_interleaved` | 按连续记录/交错文件组织独立字段修改 |
| `array_element` / `array_replace` | 单元素路径修改/完整数组字段替换 |
| `dict_element` / `dict_replace` | 单字典值路径修改/完整字典字段替换 |
| `nested_field` / `option_field` | 内联对象字段修改/Some 与 None 切换 |
| `insert_record` / `delete_record` | 新增无入边记录/删除预先插入的无入边记录 |
| `rename_record` | 修改记录 ID 并更新相关引用和模板 |
| `swap_records` | 同文件交换前两条记录，每文件至少两条记录 |
| `transfer_record` | 将第一条记录迁到第二个文件，至少两个文件 |
| `dimension_set` | 修改独立小文件 `dimensions/language/bench.cfd` 中的 `language[bench]` 覆盖；预热通常承担首次创建 |
| `dimension_add` / `dimension_clear` | 在独立小文件 `bench.cfd` 中添加/清除显式覆盖，状态准备不计时 |
| `dimension_existing_set` / `dimension_existing_clear` | 显式选择；修改/清除既有 `dimensions/language/variants.cfd` 的 `language[v0]` 第零条名称覆盖，要求 `MATRIX_VARIANTS > 0` |
| `dimension_lookup` | 查询现有 language 覆盖，含返回值复制，不等于完整模型投影 |
| `external_conflict` | 外部写入注释后拒绝旧会话写入，验证外部内容保留 |

公共 Runtime 字段事务没有独立的数组插入/移动或字典增删操作，上述集合场景测量完整值替换及元素路径写入。CLI 操作矩阵另测专用集合操作。完整模型维度投影可使用既有 `engine_perf` 的 model 模式。

每个操作使用独立夹具和会话。每个样本的请求、准备与复原在计时外；计时包含公开 API 调用和内部临时对象析构，返回结果析构和正确性断言在计时外。热写入使用持续变化的值；结构操作在样本间复原。新会话加载保留操作系统文件缓存，不解释为机器冷启动。

`dimension_existing_set` / `dimension_existing_clear` 直接使用夹具中包含全部 language 变体的既有文件，`MATRIX_VARIANTS=10` 配合 `MATRIX_STRIDE=100` / `1` 分别覆盖稀疏/密集布局。每次写入都通过预期原值校验，并断言实际仅写入该文件；输出记录文件前后字节数、变体数和初始覆盖条数。每个样本修改后，在计时外从磁盘独立重载并比较全部基础字段；两种维度的全部记录、全部变体逐项核对显式值与缺失状态，验证其他覆盖保持正确。复原同样使用公共写入 API，逐次校验恢复值，并在组末对恢复状态再次完整重载。清除再插入允许源文本格式变化，恢复以语义一致为准。阶段事件立即刷新，便于区分 API 计时、完整校验与复原阶段；进程墙钟包含这些计时外工作。

输出逐次 `samples_ms`，算术中位数、范围，以及至少 20 个样本时的 nearest-rank P95；查询给出 `operations_per_sample`。加载/刷新 `stats` 为最后一次返回代次的执行工作量。写入 `stats` 同时提供 `generation_changed`、实际写入文件数和 `last_published_generation`；同值及冲突操作未发布新代次，保留的旧代次计数不能解释为本次执行工作量。写入组结束独立从磁盘完整加载并比较所有基础字段与无诊断，另验证保留的旧记录快照。维度写入同时核对覆盖值/缺失状态；冲突场景验证拒绝与文件保留。进程峰值内存应由外部运行器采样，包含夹具、会话、校验快照及分配器保留内存。

建议先在 1,000 条运行完整操作矩阵，再在 10,000 条验证常见写入/结构操作；100,000 条重点测加载、字段更新、文件布局、批量、稀疏/集中依赖以及单文件处理成本。20/100 字段、8/1000 集合、0/1/10 变体作为独立对照，避免将所有参数无意义地全笛卡尔积组合。大规模慢场景小样本只描述中位数和范围。

`load_budget_rejected` 独立描述结构容量边界，不进入成功加载指标。当前默认每文件语法预算为 1,000,000 节点；20 字段、8 个数组元素的基础夹具每条记录计 59 节点，单文件可完整解析 16,949 条。以 `MATRIX_RECORDS=17000`、`MATRIX_FILES=1`、`MATRIX_FIELDS=20`、`MATRIX_COLLECTION=8`、`MATRIX_TOPOLOGY=star`、`MATRIX_TEMPLATES=0`、`MATRIX_WARMUPS=0`、`MATRIX_SAMPLES=1` 冒烟，隔离节点拒绝及诊断映射成本。`ring` 加模板的对照同时包含截断后的引用失效传播，单独记录。该操作在计时外立即刷新 `sample_started` / `sample_completed` 事件，标注 warmup/sample 阶段，并验证 `CFD-TEXT-Syntax` 的具体节点上限消息。保留记录数和后续引用诊断用于解释部分加载结果，不要求完整输入均成功加载。
