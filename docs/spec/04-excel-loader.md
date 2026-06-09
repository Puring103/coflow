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

加载结果为 `DataModel`，结构定义见 [02-data-model.md](02-data-model.md)。

模型构建分为以下几个阶段，任一阶段出错则停止并报告错误。CFT check 诊断不属于模型构建硬错误；`load_excel` 在模型构建完成后收集并返回 check 诊断。

### 第一阶段：解析 Excel 结构

读取 `ExcelSource` 指定的 `.xlsx` 文件，按 sheet 配置确定每个 sheet 对应的 CFT 类型。

每个 sheet 的**第一行**为列名行，后续行为数据行。空行跳过。

### 第二阶段：逐行解析记录

对每个数据行，按列名（经过 `columns` 映射后）找到对应字段，根据字段的 CFT 类型 schema-guided 解析单元格内容（见第 3 节）。

解析结果构造为 `Record`，加入对应 `Table` 的 `records` 列表。同时将 `@id` 字段值注册到该类型的 `primary_index`，重复 ID 立即报错。

如果该类型属于带 `@id` 的继承树，加载器还要把记录注册到相关 `inheritance_index`。同一继承树索引中的 ID 必须唯一，兄弟子类之间重复 ID 也立即报错。

同一类型的 records 按 `ExcelSource` 中 sources 顺序追加（见 [02-data-model.md](02-data-model.md)）。

### 第三阶段：解析跨表引用

遍历所有 Record，将 `@ref` 字段的值（string 或 int）按目标类型的赋值兼容范围查找，替换为 `Value::Ref { id, target }`。

- `@ref(TypeName)` 中 TypeName 是 `sealed type` 或无子类的普通 `type` 时，在该类型的 `primary_index` 中查找
- `@ref(TypeName)` 中 TypeName 是 `abstract type` 或有子类的普通 `type` 时，在对应 `inheritance_index` 中查找
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

---

## 4. API 语义

- `load_excel_model` builds a data model and does not run CFT checks.
- `load_excel` builds a data model and runs CFT checks.

---

## 5. 错误阶段

| 阶段 | 错误类型 |
|------|---------|
| Excel 解析 | 文件不存在、sheet 不存在、`type` 指定的类型未定义 |
| 记录解析 | 列名找不到对应字段、单元格值类型不匹配、`@id` 重复、继承树 ID 重复、字典 key 重复、多态字段缺少类型标记 |
| 跨表引用解析 | `@ref` 目标类型不存在、目标 ID 找不到 |
| check 执行 | 条件为假、类型错误、越界 |
