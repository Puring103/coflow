# Excel 加载器规格

**依赖文档**：[01-cft.md](01-cft.md)、[02-schema-api.md](02-schema-api.md)、[02-data-model.md](02-data-model.md)、[03-cell-value.md](03-cell-value.md)

`coflow-loader-excel` 是低层加载 crate。它接收已经编译完成的 `CftContainer`，以及已经解析出的 `ExcelSource` 值，将 Excel 数据按 CFT schema 加载为来源无关的输入记录。加载过程完全 schema-guided，表头、record key、单元格值和字段类型解析均依赖 CFT 类型定义。

Excel 的表格语义由 `coflow-loader-table-core` 承担。Excel loader 只负责把 `.xlsx` 单元格读取并转换为文本表格，再把表头、key、columns、`@expand`、record 和诊断映射交给共享 table loader。飞书电子表格等远端表格源也使用同一套 table loader。

`coflow-loader-excel` 不发现项目文件，不解析 `coflow.yaml`，也不负责 CLI 编排。它实现 `coflow-api::DataLoader`，由宿主通过 `ProviderRegistry` 调用；项目生命周期由 `coflow-engine` 编排，导出/codegen 产物落盘由 CLI 宿主负责。

---

## 目录

1. [输入边界](#1-输入边界)
2. [加载流程](#2-加载流程)
3. [单元格值解析](#3-单元格值解析)
4. [API 语义](#4-api-语义)
5. [错误阶段](#5-错误阶段)

---

## 1. 输入边界

Excel 加载器的输入是：

- 已经成功 `compile` 的 `CftContainer`
- 已经由上层项目管线解析出的 `ExcelSource` 值

`ExcelSource` 描述一个或多个 Excel 文件、sheet 到 CFT type 的映射、可选 record key 列名，以及可选的列名到字段名映射。路径解析、配置文件读取、文件发现和命令行选项合并不属于 `coflow-loader-excel` 的职责。

如果 `ExcelSource.sheets` 为空，加载器默认加载 workbook 中全部 sheet：sheet 名作为 CFT 类型名，表头文本作为字段名。需要重命名 sheet、覆盖类型名、覆盖 key 列或映射显示列头时，上层传入显式 `sheets` 配置。`key` 省略时默认使用 `id`、`Id` 或 `ID` 表头列；`columns` 是列头重命名映射，不限制未列出的字段。

---

## 2. 加载流程

加载结果为 `CfdInputRecord` 列表，后续由 `coflow-engine` 统一构建 `CfdDataModel` 并执行 check。数据模型结构定义见 [02-data-model.md](02-data-model.md)。

加载分为以下几个阶段。Excel 文件、工作表、表头和单元格问题会尽量跨数据源、工作表和数据行聚合；但某个工作表的表头无法可靠映射时，该工作表的数据行会被跳过。

CFT check 诊断不属于 Excel loader 职责；loader 只负责把 workbook 转成带来源定位的 input records。check 由 `coflow-engine` 在 data model 构建后统一执行。

### 第一阶段：解析 Excel 结构

读取 `ExcelSource` 指定的 `.xlsx` 文件，按 sheet 配置确定每个 sheet 对应的 CFT 类型。

每个 sheet 的**第一行**为列名行，后续行为数据行。空行跳过。

列名为 `#` 的列是可选导入控制列，不映射到 CFT 字段，也不参与未知
字段检查。数据行中该列的文本值去除首尾空白后等于 `##` 时，整行跳过，
不会读取 `id`，也不会解析任何字段。其他值没有特殊含义。

`@expand` 字段按被展开类型的字段声明顺序消费父字段列和后续相邻列。父字段列承载第一个子字段；
后续相邻列必须连续，且表头必须为空。Excel 合并表头通常只有左上角单元格保留文本，
被合并覆盖的后续单元格在读取时表现为空表头，因此等价于合法的 `@expand` 布局。
若后续相邻列写了任何非空表头，加载器会报告 `EXCEL-COLUMN` 并跳过该 sheet 的数据行，
避免普通业务列被静默当成 `@expand` 子字段吞掉。

Excel 原生单元格先转换为单元格文本，再交给 schema-guided cell parser：
文本、整数、浮点和布尔单元格会被转换为对应文本；整数值的浮点单元格会去掉
`.0` 后缀。Excel error、原生 date/time、typed ISO date/time 和 typed ISO
duration 单元格不进入 cell parser，直接报告 `EXCEL-CELL`。日期、时间和持续
时间如果要按 Coflow 语法解析，应在 Excel 中保存为普通文本。

### 第二阶段：逐行解析记录

对每个数据行，先读取 key 列作为 record key；默认 key 列名为 `id`、`Id` 或 `ID`，也可由 `key` 配置覆盖。显式配置 `key` 时按配置值精确匹配表头。key 列不映射到 CFT 字段，且 CFT 禁止声明名为 `id`、`Id` 或 `ID` 的字段。随后按列名（经过 `columns` 映射后）找到对应字段，根据字段的 CFT 类型 schema-guided 解析单元格内容（见第 3 节）。

解析结果先构造为与来源无关的 `CfdInputRecord`，后续由 engine 交给数据模型构建器生成
`CfdRecord`，加入对应 `CfdTable.records` 列表。同时将 record key 注册到该类型的 `primary_index`，重复 key 作为数据模型诊断报告。空 key 或非法 key 在 Excel 阶段报告。

如果该类型属于继承树，加载器还要把记录注册到相关 `inheritance_index`。同一继承树索引中的 key 必须唯一，兄弟子类之间重复 key 也立即报错。

同一类型的 records 按 `ExcelSource` 中 sources 顺序追加（见 [02-data-model.md](02-data-model.md)）。

### 第三阶段：交给 engine 构建模型

Excel loader 输出 `CfdInputRecord` 后停止。跨表引用解析、重复 key 检查、继承索引、path ref 解析和 CFT check 由 `coflow-engine` 统一交给 `coflow-data-model` 与 `coflow-checker` 处理。

---

## 3. 单元格值解析

单元格值语法详见 [03-cell-value.md](03-cell-value.md)。

解析器是完全 schema-guided 的，每个单元格的目标类型由对应列的 CFT 字段类型确定。

**特殊情况：**

| 单元格内容 | 语义 |
|-----------|------|
| 空单元格 | 字段有默认值时使用默认值；无默认值且非 nullable 时报错 |
| `_` | 同空单元格 |
| `null` | 显式填 null；字段类型必须是 `T?`，否则报错 |
| `@Type.key` | 对象字段的显式记录引用；目标类型为 string 时保留为普通字符串 |
| `&key` | 对象字段的直接记录引用简写；目标类型为 string 时保留为普通字符串 |

`#` 控制列只在整行导入决策中生效，不作为普通单元格值交给字段解析器。

---

## 4. API 语义

- `collect_input_records(schema, sources)`：读取 Excel，执行 schema-guided table/cell parsing，返回 `ExcelInputRecords { records }`。
- `ExcelLoader` 实现 `coflow-api::DataLoader`，由 engine 通过 `ProviderRegistry` 调用。
- `ExcelWriter` 实现 `coflow-api::DataWriter`，由 editor/engine 写回流程通过 `ProviderRegistry` 调用。
- Excel crate 不公开 `load_excel_model` / `load_excel` 这类 standalone runtime facade。

---

## 5. 错误阶段

| 阶段 | 错误类型 |
|------|---------|
| Excel 解析 | 文件不存在、sheet 不存在、`type` 指定的类型未定义 |
| 记录解析 | 缺少 `id` 列、空 record key、非法 record key、列名找不到对应字段、单元格值类型不匹配、record key 重复、继承树 key 重复、字典 key 重复、多态字段缺少类型标记 |
| 模型/check | 重复 key、引用目标不存在、路径字段/索引不存在、check 条件失败；这些由 engine 汇总为 data model/check 诊断 |
