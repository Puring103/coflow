# 数据模型

Coflow 的数据模型是所有数据源汇合后的统一运行时表示。Excel、CSV、CFD、飞书/Lark 表格和维度隐式 source 都会先被 Provider 转换成来源无关的 input records，再由 DataModel 统一处理默认值、类型检查、引用解析、继承索引和业务校验。

数据模型不保留数据源格式差异。导出、代码生成、编辑器视图和 `check {}` 都基于同一个模型工作。

## 结构

核心结构可以理解为：

```text
CfdDataModel
  tables              # TypeName -> CfdTable
  domain_index        # TypeName -> TypeId / DomainId
  records             # 所有顶层 records
  ref_edges           # 直接 &Type 引用边
  spread_edges        # ...&key spread 来源边

CfdTable
  type_name
  records             # RecordId 列表，保持数据源顺序
  primary_index        # record key -> RecordId

CfdRecord
  key                 # record key
  object              # 顶层 record 的对象值

CfdObject
  actual_type          # 运行时实际类型
  fields               # FieldName -> CfdValue
```

`records` 是集中存储。table 和继承索引只保存 `CfdRecordId`，消费者通过 model 查询记录，而不是复制记录内容。

## Value 类型

`CfdValue` 覆盖 CFT 支持的数据形状：

| 类型 | 说明 |
| --- | --- |
| `Null` | nullable 字段的空值 |
| `Bool` | 布尔值 |
| `Int` | 64 位整数 |
| `Float` | 有限 `f64`，不允许 `NaN` 或无穷值 |
| `String` | 字符串 |
| `Enum` | 枚举值，携带 enum 名、底层整数值和可选 variant 名 |
| `Object` | 内联对象，无独立 record identity |
| `Ref` | 指向顶层 record 的引用，值本身只保留目标 key |
| `Array` | 有序数组 |
| `Dict` | 保持插入顺序的字典 |

字典 key 只允许 `string`、`int` 和 enum。重复字典 key 是数据错误，不会后写覆盖。

## Object 与 Ref

`Object` 表示内联对象，只属于所在记录或字段值，不可被其他记录引用。

`Ref` 表示对顶层 record 的共享引用。构建模型时，Coflow 会把字段类型为 `&Type` 的输入值 `&key` 解析成目标记录；找不到目标或类型不兼容时报告引用诊断。

字段类型会约束输入形态：

| 字段类型 | DataModel 行为 |
| --- | --- |
| `&Item` | 该字段位置必须是记录引用，拒绝内联对象 |
| `Item` | 该字段位置必须是内联对象，拒绝记录引用 |
| `[&Item]` / `{string: &Item}` | 集合元素或字典 value 必须是记录引用 |

这些约束会沿数组元素和字典 value 递归应用。`null` 仍按 nullable 规则处理。

## actual_type

每条 record 都有 `actual_type`。它表示运行时实际 CFT 类型，用于多态判断、继承 check、导出和代码生成。

| 场景 | actual_type 来源 |
| --- | --- |
| 顶层 record | 数据源声明的 record 类型 |
| 非多态内联对象 | 字段声明类型 |
| 多态对象 | 输入中的具体类型标记，例如 `CurrencyReward{...}` |

`actual_type` 参与：

- `check {}` 中的 `is` 类型判断。
- 从父类到实际类型的 check 执行顺序。
- JSON 导出中的 `$type` 判断。
- C# codegen 的类型分发。

## 继承与引用范围

当字段声明为 `&Type` 时，引用查找会在该 type 所属继承命名域内进行：

| 引用类型 | 可引用范围 |
| --- | --- |
| `&Reward`，其中 `Reward` 是 abstract type | 所有具体子类 |
| `&Item`，其中 `Item` 有子类 | 自身和所有子类 |
| `&Stats`，其中 `Stats` 是 sealed type 或无子类普通 type | 仅自身 |

同一继承连通分量内 record key 必须唯一。无继承关系的 type 仍可复用相同 key。

子类 record 可以满足父类字段引用；父类 record 不能满足子类字段引用。

## 引用索引

直接 `&Type` 引用会生成 `RefEdge`。`RefEdge` 记录 host record、字段路径、期望 type、继承命名域、目标 key 和目标 record id。

```text
RefSite(host_record, field_path) -> RefEdge
```

DataModel 同时维护：

- `ref_by_site`：按字段位置查目标。
- `ref_by_host`：按 record 枚举直接出边，供图视图使用。
- `ref_by_target`：按目标 record 枚举入边，供 rename 和删除检查使用。
- `spread_by_site` / `spread_by_source`：独立描述 `...&key` 来源，不进入直接引用图。

## 数据源顺序

同一 type 的 records 按稳定顺序追加：

1. `coflow.yaml` 中 `sources` 的顺序。
2. 同一表格 source 内 `sheets` 的顺序。
3. 同一 sheet 内的行顺序。
4. 同一 CFD 文件内记录出现顺序。

这个顺序会影响 `data list`、导出文件中的记录顺序，以及编辑器展示顺序。

## 默认值与必填字段

字段缺省时：

- 有 CFT 默认值：使用默认值。
- 是 nullable 字段：可以使用 `null`。
- 没有默认值且非 nullable：报告 `CFD-DATA-006`。

默认值由 schema 编译阶段确定。DataModel 构建时只应用已编译的默认值，不重新解释 CFT 源文本。

写入接口把字段值视为完整值：每一层内联 object 都必须包含所有没有 CFT 默认值的字段。局部片段验证只检查已提供字段，只用于尚未提交的路径级构造过程，不能直接通过 Provider 写入。

## Singleton

`@singleton` type 在 DataModel 阶段执行约束：

| 规则 | 诊断 |
| --- | --- |
| 该 type 必须恰好有一条 record | `CFD-DATA-015` |
| record key 必须存在且是合法 CFT 标识符 | `CFD-DATA-016` |
| 不同 `@singleton` type 的 record key 不能撞名 | `CFD-DATA-017` |

`@singleton` type 不能作为普通引用字段类型使用。这个限制在 CFT schema 阶段检查。

## 维度字段

`@localized` 字段会被 engine 作为 `language` 维度字段处理。DataModel 看到的是已经注入的合成 type 和隐式 source，因此维度数据仍走普通记录、字段、引用和 check 流程。

维度 record key 仍遵守通用 record key 规则。当前没有单独的本地化 key 规则。

## 与 Provider 的边界

Provider 只负责把来源格式转成 input records，并提供来源定位：

- Excel / CSV / Lark 负责表头、行、单元格文本读取。
- CFD 负责文本记录解析。
- 维度生成负责维护隐式 source 文件。

以下规则由 DataModel 统一处理：

- 默认值。
- 必填字段。
- 字段类型匹配。
- 多态可赋值。
- 字典 key 去重。
- record key 唯一性。
- `&Type` 记录引用解析。
- `@singleton` 约束。

这样不同来源可以互相引用，并且所有来源得到一致的校验结果。
