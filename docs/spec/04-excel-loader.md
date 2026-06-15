# Excel 加载器规格

**依赖文档**：[01-cft.md](01-cft.md)、[02-schema-api.md](02-schema-api.md)、[02-data-model.md](02-data-model.md)、[03-cell-value.md](03-cell-value.md)

`coflow-loader-excel` 是低层加载 crate。它接收已经编译完成的 `CftContainer`，以及已经解析出的 `ExcelSource` 值，将 Excel 数据按 CFT schema 加载为结构化数据模型。加载过程完全 schema-guided，所有类型解析、单元格值解析和跨表引用解析均依赖 CFT 类型定义。

`coflow-loader-excel` 不发现项目文件，不解析 `coflow.yaml`，也不负责 CLI 编排。项目配置、schema 发现、Excel source 定义和导出/codegen 调用由项目加载与 CLI 层负责。

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

`ExcelSource` 描述一个或多个 Excel 文件、sheet 到 CFT type 的映射，以及可选的列名到字段名映射。路径解析、配置文件读取、文件发现和命令行选项合并不属于 `coflow-loader-excel` 的职责。

---

## 2. 加载流程

加载结果为 `CfdDataModel`，结构定义见 [02-data-model.md](02-data-model.md)。

模型构建分为以下几个阶段。Excel 文件、工作表、表头和单元格问题会尽量跨数据源、工作表和数据行聚合；但某个工作表的表头无法可靠映射时，该工作表的数据行会被跳过。只有前置结构和单元格解析都没有硬错误时，加载器才进入数据模型构建、引用解析和后续 check。

CFT check 诊断不属于模型构建硬错误；`load_excel` 在模型构建完成后收集并返回 check 诊断。

### 第一阶段：解析 Excel 结构

读取 `ExcelSource` 指定的 `.xlsx` 文件，按 sheet 配置确定每个 sheet 对应的 CFT 类型。

每个 sheet 的**第一行**为列名行，后续行为数据行。空行跳过。

列名为 `#` 的列是可选导入控制列，不映射到 CFT 字段，也不参与未知
字段检查。数据行中该列的文本值去除首尾空白后等于 `##` 时，整行跳过，
不会读取 `id`，也不会解析任何字段。其他值没有特殊含义。

Excel 原生单元格先转换为单元格文本，再交给 schema-guided cell parser：
文本、整数、浮点和布尔单元格会被转换为对应文本；整数值的浮点单元格会去掉
`.0` 后缀。Excel error、原生 date/time、typed ISO date/time 和 typed ISO
duration 单元格不进入 cell parser，直接报告 `EXCEL-CELL`。日期、时间和持续
时间如果要按 Coflow 语法解析，应在 Excel 中保存为普通文本。

### 第二阶段：逐行解析记录

对每个数据行，先读取特殊 `id` 列作为 record key；`id` 不映射到 CFT 字段，且 CFT 禁止声明名为 `id` 的字段。随后按列名（经过 `columns` 映射后）找到对应字段，根据字段的 CFT 类型 schema-guided 解析单元格内容（见第 3 节）。

解析结果先构造为与来源无关的 `CfdInputRecord`，后续由数据模型构建器生成
`CfdRecord`，加入对应 `CfdTable.records` 列表。同时将 record key 注册到该类型的 `primary_index`，重复 key 作为数据模型诊断报告。空 key 或非法 key 在 Excel 阶段报告。

如果该类型属于继承树，加载器还要把记录注册到相关 `inheritance_index`。同一继承树索引中的 key 必须唯一，兄弟子类之间重复 key 也立即报错。

同一类型的 records 按 `ExcelSource` 中 sources 顺序追加（见 [02-data-model.md](02-data-model.md)）。

### 第三阶段：解析跨表引用

遍历所有 Record，将对象字段中的 `@Type.key`、`&key` 或 `@Type.key.path[index]` 输入按目标类型解析，替换为 `Value::Ref { key, target }` 或路径结果值。`Type` 必须是 CFT 类型名，`key` 必须是 string identifier record key。`&key` 是直接引用简写，使用当前字段期望类型作为查找根类型，不支持路径。

- `Type` 是 `sealed type` 或无子类的普通 `type` 时，在该类型的 `primary_index` 中查找
- `Type` 是 `abstract type` 或有子类的普通 `type` 时，在对应 `inheritance_index` 中查找
- 引用前缀 `Type` 还必须能赋给字段声明类型；子类引用可用于父类字段，父类引用不能用于子类字段
- `&key` 按字段声明类型执行同样的查找规则；如需跨根类型路径访问，必须写 `@Type.key.path[index]`
- 允许循环引用（A.ref → B，B.ref → A）；两遍设计天然支持，不会无限递归
- 找不到目标则报错

### 第四阶段：执行 check

`load_excel_model` 不执行 CFT check；它只构建数据模型并解析引用。

`load_excel` 在 `load_excel_model` 构建数据模型之后执行 CFT check 块校验，收集全部错误后一次性返回。check 错误不影响数据模型的构建结果，由宿主决定是否使用含错误的数据。

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

- `load_excel_model(schema, sources)`：加载 Excel、构建 `CfdDataModel` 并解析引用；不执行 CFT `check {}`。
- `load_excel(schema, sources)`：在 `load_excel_model` 的模型构建语义之上执行 CFT `check {}`，返回 `ExcelLoadOutput { model, check_diagnostics }`。check 失败不会丢弃已经构建完成的 model。

---

## 5. 错误阶段

| 阶段 | 错误类型 |
|------|---------|
| Excel 解析 | 文件不存在、sheet 不存在、`type` 指定的类型未定义 |
| 记录解析 | 缺少 `id` 列、空 record key、非法 record key、列名找不到对应字段、单元格值类型不匹配、record key 重复、继承树 key 重复、字典 key 重复、多态字段缺少类型标记 |
| 跨表引用解析 | 引用目标类型不存在、目标 key 找不到、路径字段/索引不存在、路径结果类型不匹配 |
| check 执行 | 条件为假、类型错误、越界 |
