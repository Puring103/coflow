# 诊断规格

Coflow 诊断是项目配置校验、CFT 编译、Excel/CFD 数据加载、data model 构建、引用解析、
`check {}` 执行、artifact preflight 和 codegen preflight 产生的结构化错误。

诊断不同于不可恢复的 CLI 错误。诊断描述 Coflow 能以结构化形式报告的项目输入
问题；CLI 错误表示 Coflow 无法可靠继续的失败，例如无法读取或解析
`coflow.yaml`。

---

## 人类可读输出

人类可读诊断写入 stderr。每个诊断之间用分隔线隔开：

```text
----------------------------------------
[CODE] [STAGE]
file    path/to/file.cft
line    12
column  5
message
  description of the problem
```

Excel 诊断可能显示 `sheet` 和 `cell`，而不只是行号/列号：

```text
----------------------------------------
[CELL-TypeMismatch] [CELL]
file    data/items.xlsx
sheet   Item
cell    B2
message
  failed to parse `Item.level` cell: expected int
```

项目级诊断不一定有源文件位置。人类可读输出中，这类诊断使用第 `1` 行、
第 `1` 列作为兜底位置。

---

## JSON 输出

`coflow check --json` 和 `coflow cft check --json` 输出：

```json
{
  "diagnostics": [
    {
      "code": "CFT-SCHEMA-006",
      "stage": "SCHEMA",
      "severity": "error",
      "message": "unknown type `Missing`",
      "path": "schema/main.cft",
      "startLine": 0,
      "startCharacter": 14,
      "endLine": 0,
      "endCharacter": 21,
      "related": []
    }
  ]
}
```

JSON 中所有位置均为零基。human 输出显示一基 line/column。

诊断字段：

| 字段 | 含义 |
| --- | --- |
| `code` | 稳定诊断码或诊断码族。 |
| `stage` | 产生诊断的管线阶段。 |
| `severity` | 当前始终为 `error`。 |
| `message` | 面向人的错误说明。 |
| `path` | 有文件位置时的文件路径。 |
| `sheet` | 适用于 Excel 诊断的 worksheet 名称。 |
| `cell` | 适用于 Excel 诊断的 A1 单元格位置。 |
| `startLine`, `startCharacter`, `endLine`, `endCharacter` | 零基源码范围。 |
| `related` | 相关位置，例如重复声明或重复 ID。 |

`related` 项具有同样的位置字段，并可包含 `label`。

---

## 诊断码族

### `CLI-ERROR`

阶段：`CLI`

不可恢复的命令级错误。这类错误不是普通结构化项目诊断，也不会聚合。

示例：

- 配置路径不存在。
- `coflow.yaml` 无法读取。
- YAML 语法无法解析。
- `coflow.yaml` 含有 YAML 反序列化阶段拒绝的未知字段。
- artifact preflight 之后发生写入失败，例如权限错误。
- codegen preflight 已通过，但 enum lockfile 格式损坏。

### `PROJECT-001`

阶段：`PROJECT`

`coflow.yaml` 已成功读取和解析之后的项目配置与命令 preflight 诊断。

示例：

- schema path 为空或不存在。
- schema 列表为空。
- source file/dir path 为空或不存在。
- source 同时设置 `file` 和 `dir`，或二者都没有设置。
- sheet 名称为空。
- sheet type override 为空。
- output type 不支持。
- output directory 为空。
- 设置了 `outputs.data.namespace`。
- `outputs.code.namespace` 为空。
- 命令需要的 output 配置缺失或类型不兼容。

项目诊断会尽量聚合。例如单次 `coflow check --json` 可以同时报告多个 schema
path、source 和 output 配置问题。

### `CFT-LEX-*`

阶段：`LEX`

`.cft` 文件词法错误，例如非法字符、非法字符串转义、字符串未闭合、非法数字
字面量。

单个 `.cft` 文件内的 lex/syntax 恢复能力有意保持有限：一个文件可能只报告
第一个阻塞解析的问题。其他 schema 文件仍会在可能时继续处理。

### `CFT-SYN-*`

阶段：`SYN`

`.cft` 文件语法错误，例如遇到非预期 token、意外 EOF、非法顶层 item、
annotation 语法错误、非法 check 语句或重复 check block。

与词法诊断类似，语法诊断可能在当前文件内停止，因为阻塞解析后没有可靠 AST。
其他 schema 文件仍可继续贡献诊断。

### `CFT-SCHEMA-*`

阶段：`SCHEMA`

解析成功后的 schema 编译诊断。该阶段会广泛聚合。

示例：

- module、type、field、enum variant 或 enum value 重复。
- 未知命名类型。
- 非法继承。
- 非法使用 `@struct`、`@expand`、`@flag`、`@display`、`@deprecated` 或
  `@keyAsEnum`。
- 旧字段级 `@id`、`@index`、`@ref`、`@IdAsEnum` 和 `@GenAsEnum` 已移除；
  应改用字段的 CFT 类型以及 `@Type.key` 或 `&key` 单元格引用。
- 声明保留字段 `id`。
- 默认值非法。
- enum value 序列非法。
- 引用目标没有 ID。
- 引用 ID 类型不匹配。

### `CFT-TYPE-*`

阶段：`TYPE`

CFT `check {}` 表达式的静态类型检查诊断。

示例：

- 未知值或字段。
- enum variant 使用非法。
- 运算符或比较类型不匹配。
- 条件不是 bool。
- 未知函数。
- 函数参数数量或类型错误。
- 非法索引。
- 非法 `is` 谓词。
- `unique` 元素类型不支持。
- 正则 pattern 非法。

### `EXCEL-OPEN`

阶段：`EXCEL`

Coflow 无法打开 workbook。

常见原因：

- 文件不是有效 Excel workbook。
- 文件被锁定或不可读。
- 路径指向非 workbook 文件。

缺失 source 文件通常会更早报告为 `PROJECT-001`。

### `EXCEL-SHEET`

阶段：`EXCEL`

workbook sheet 级诊断。

示例：

- 配置的 sheet 不存在。
- sheet 无法读取。
- sheet 为空。

Coflow 会在可能时继续处理其他 workbook 和其他 sheet。

### `EXCEL-TYPE`

阶段：`EXCEL`

sheet 映射到未知 CFT 类型。可能来自显式 `type`，也可能来自省略 `type` 时使用
的 sheet 名称。

### `EXCEL-COLUMN`

阶段：`EXCEL`

表头映射诊断。

示例：

- 表头映射到未知字段。
- 两列映射到同一 CFT 字段。
- `@expand` 没有足够相邻列覆盖所有子字段。
- `@expand` 后续相邻列的非空表头不是预期子字段名，可能会吞掉普通业务列。

如果某个 sheet 有表头诊断，Coflow 会跳过该 sheet 的数据行解析，因为 row
value 已无法可靠映射。其他 sheet 继续处理。

### `EXCEL-ID`

阶段：`EXCEL`

record key 列诊断。

示例：

- 可加载类型的 sheet 没有 `id` 列。
- 数据行的 `id` 单元格为空。
- `id` 单元格值不是合法 record key identifier。

被可选 `#` 导入控制列跳过的行不需要合法 `id` 单元格。

### `EXCEL-CELL`

阶段：`EXCEL`

不支持的原始 Excel 单元格值。

示例：

- Excel error 单元格。
- Excel 原生 date/time 单元格。
- 以 typed Excel value 表示的 ISO duration/date 单元格，而不是普通文本。

需要由 Coflow schema-guided cell parser 解析的值，应在 Excel 中保存为文本。

### `CELL-*`

阶段：`CELL`

schema-guided cell parser 诊断。具体后缀来自 cell value parser，例如
`CELL-TypeMismatch`。

示例：

- `int` 字段中填写 `not_int`。
- array、dict 或 object 语法错误。
- enum value 非法。
- string escaping 非法。
- polymorphic object type marker 非法。

表头有效时，cell 诊断会跨整个 sheet 收集。

### `CFD-TEXT-*`

阶段：`CFD`

`.cfd` 文本解析和 schema-guided 转换诊断。具体后缀来自 CFD 文本加载器，例如
`CFD-TEXT-Syntax`。

示例：

- CFD 语法错误。
- 文本记录或 object 使用未知类型。
- 文本字段名不存在。
- enum variant 非法。
- 引用字段没有使用 `@Type.key`、`&key` 或 `null` 等合法引用形式。

### `CFD-DATA-*`

阶段：`DATA`

Excel/CFD source 已解析后的 data model 构建诊断。

诊断码包括：

| 诊断码 | 含义 |
| --- | --- |
| `CFD-DATA-001` | 未知 record 或 object type |
| `CFD-DATA-002` | 直接使用 abstract record type |
| `CFD-DATA-003` | 缺少 polymorphic object type |
| `CFD-DATA-004` | object actual type 不可赋给声明类型 |
| `CFD-DATA-005` | 未知字段 |
| `CFD-DATA-006` | 缺少必填字段 |
| `CFD-DATA-007` | value 类型不匹配 |
| `CFD-DATA-008` | enum variant 非法 |
| `CFD-DATA-009` | dict key 重复 |
| `CFD-DATA-010` | 缺少 ID 字段 |
| `CFD-DATA-011` | ID 重复 |
| `CFD-DATA-012` | polymorphic range 内 ID 重复 |
| `CFD-DATA-013` | record key identifier 非法 |

data model 诊断在 data model 阶段内聚合。data model 无效时，引用解析和 check
不会运行。

### `CFD-REF-*`

阶段：`REF`

引用解析诊断。

| 诊断码 | 含义 |
| --- | --- |
| `CFD-REF-001` | 找不到被引用的目标记录 |

引用解析失败时，check 不会运行。

### `CFD-CHECK-*`

阶段：`CHECK`

运行期 `check {}` 诊断。

| 诊断码 | 含义 |
| --- | --- |
| `CFD-CHECK-001` | check 条件结果为 false |
| `CFD-CHECK-002` | check 求值运行期类型错误 |
| `CFD-CHECK-003` | null access |
| `CFD-CHECK-004` | index out of bounds |
| `CFD-CHECK-005` | missing dictionary key |
| `CFD-CHECK-006` | `min` / `max` 没有非 null 值 |

硬运行期错误会停止当前 check block，但 Coflow 继续处理同一 record 后续可处理的
check、其他 record 和嵌套 object check。普通 false 条件、`any` / `none`
失败，以及多个 record 上的失败会被聚合。

### `CODEGEN-CSHARP-001`

阶段：`CODEGEN`

C# codegen preflight 诊断，发生在任何生成文件被修改之前。

示例：

- namespace 非法。
- C# type、enum、enum variant、member 或 database class 名称非法。
- 生成文件名冲突。
- 生成 member 名冲突。
- 配置的 database 文件名冲突。
- `@keyAsEnum` 生成 variant 非法。
- `@keyAsEnum` variant value 重复。

存在这些诊断时，Coflow 不读写 enum lockfile，不清理旧 `.cs` 文件，也不生成
新的 `.cs` 文件。

### `ARTIFACT-001`

阶段：`ARTIFACT`

artifact preflight 诊断。目前只报告输出路径已经存在且不是目录的情况。

这是非写入 preflight。Coflow 不在这个检查中创建目录或测试权限。权限错误和
真实写入失败仍是运行期 `CLI-ERROR`。

---

## 非阻塞收集规则

“非阻塞”表示 Coflow 在不依赖无效中间数据的前提下继续收集诊断；不表示会在
输入无效时生成产物。

| 阶段 | 是否继续收集 | 可能停止的原因 |
| --- | --- | --- |
| 配置文件发现/读取/YAML 解析 | 否 | 没有有效项目配置 |
| 已解析项目配置校验 | 是 | 独立配置字段可一起检查 |
| Schema path 发现 | 是 | 可报告多个 path 错误 |
| 单个 CFT 文件 lex/syntax | 有限 | 阻塞解析后没有可靠 AST |
| 多个 CFT 文件 | 是 | 其他文件仍可解析 |
| Schema/type 编译 | 是 | compiler 可聚合语义诊断 |
| Excel workbook/sheet 发现 | 是 | 其他 workbook/sheet 仍可处理 |
| Excel 表头映射 | 表头内是；sheet rows 会在表头错误时跳过 | rows 无法可靠映射 |
| Excel cell 解析 | 是 | 有效表头允许独立解析 cell |
| CFD 文本解析 | 文件间是；单文件内有限 | 阻塞解析后没有可靠输入记录 |
| Data model 输入校验 | 子阶段内是 | 无效 model 阻塞引用和 check |
| 引用解析 | 子阶段内是 | 未解析引用阻塞 check |
| Check 运行期 | 跨 block/record 是 | 硬错误只停止当前 block |
| Codegen preflight | 是 | 独立命名检查可聚合 |
| Artifact preflight | 是 | 输出路径检查独立 |
| Artifact 写入 | 否 | 部分写失败是运行错误 |

---

## 产物安全

会写产物的命令都有门禁：

- `build`：存在 project、schema、数据加载、data model、reference、check、codegen
  preflight 或 artifact preflight 诊断时，什么都不写。
- `export`：导出前存在诊断时，什么都不写。
- `codegen`：存在 schema、project、codegen preflight 或 artifact 诊断时，
  什么都不写。

preflight 后发生的真实 filesystem 写入失败报告为 `CLI-ERROR`，因为 Coflow
无法可靠继续或聚合这类失败。

---

## 诊断阅读顺序

建议按以下顺序修复：

1. 先修 `CLI-ERROR`；命令还未进入结构化校验。
2. 修 `PROJECT-001`；配置问题可能阻止后续阶段。
3. 修 `CFT-*`；schema 错误会阻塞数据加载和 artifact 阶段。
4. 修 `EXCEL-*`、`CELL-*` 和 `CFD-TEXT-*`；这些错误会阻塞 data model 构建。
5. 修 `CFD-DATA-*` 和 `CFD-REF-*`；这些错误会阻塞 check 和 export。
6. 修 `CFD-CHECK-*`；数据结构有效，但违反业务规则。
7. 修 `CODEGEN-*` 和 `ARTIFACT-*`；数据可能有效，但无法安全生成产物。

与编辑器、CI 或自定义工具集成时，优先使用 JSON 输出。
