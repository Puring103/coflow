# 诊断与错误码

Coflow 诊断用于报告项目配置、CFT schema、数据源、DataModel、引用、业务校验、代码生成和产物写入中的结构化错误。

诊断不同于不可恢复的 CLI 错误。诊断表示 Coflow 已经进入可定位的项目检查阶段，可以给出 code、stage、message 和 source location；CLI 错误通常表示配置文件无法读取、命令参数无法解析等更早期失败。

## 输出形式

人类可读诊断写到 stderr：

```text
----------------------------------------
[CFT-SCHEMA-006] [SCHEMA]
file    schema/main.cft
line    12
column  5
message
  unknown type `Missing`
```

表格诊断可能包含 sheet 和 cell：

```text
----------------------------------------
[CELL-TypeMismatch] [CELL]
file    data/items.xlsx
sheet   Item
cell    B2
message
  failed to parse `Item.level` cell: expected int
```

## JSON 输出

`coflow check --json` 和 `coflow cft check --json` 输出结构化诊断：

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

JSON 中的行列位置是零基；human 输出显示为一基。

| 字段 | 说明 |
| --- | --- |
| `code` | 诊断码 |
| `stage` | 产生诊断的阶段 |
| `severity` | 严重级别，当前主要为 `error` |
| `message` | 错误说明 |
| `path` | 文件路径 |
| `sheet` | 表格 sheet 名 |
| `cell` | 表格 A1 单元格 |
| `startLine` / `startCharacter` | 起始位置 |
| `endLine` / `endCharacter` | 结束位置 |
| `related` | 相关位置，例如重复声明或重复 ID |

## 阶段

| Stage | 说明 |
| --- | --- |
| `CLI` | 命令级错误 |
| `PROJECT` | 项目配置和命令 preflight |
| `LEX` | CFT 词法 |
| `SYN` | CFT 语法 |
| `SCHEMA` | CFT schema 编译 |
| `TYPE` | CFT check 表达式静态类型检查 |
| `EXCEL` | Excel source、workbook、sheet、column、cell |
| `CSV` | CSV source 或写回 |
| `CFD` | CFD source、文本解析或写回 |
| `LARK` | 飞书/Lark source、鉴权、读取或写回 |
| `CELL` | 表格单元格值解析 |
| `DATA` | DataModel 构建 |
| `REF` | 记录引用和路径引用解析 |
| `CHECK` | CFT `check {}` 运行期校验 |
| `CODEGEN` | 代码生成 preflight |
| `ARTIFACT` | 输出目录、staging、commit、lockfile |
| `WRITE` | 通用 writer 能力错误 |

## 命令级与项目配置

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `CLI-ERROR` | `CLI` | 不可恢复的命令级错误，例如配置文件无法读取、YAML 无法解析、命令参数无法进入结构化检查 |
| `PROJECT-001` | `PROJECT` | 已解析项目配置或命令 preflight 诊断，例如路径不存在、source 配置非法、output 缺失或类型不兼容 |
| `DIM-CONFIG-001` | `PROJECT` | schema 中存在 `@localized` 字段，但未配置 `dimensions.language` |
| `DIM-CONFIG-002` | `PROJECT` | `dimensions.language.variants` 为空、包含 `default`、包含重复项或不是合法 CFT 标识符 |
| `DIM-CONFIG-003` | `PROJECT` | `dimensions.language.out_dir` 缺失 |
| `DIM-SOURCE-001` | `PROJECT` / engine | 维度文件生成或隐式 source 注册失败，例如无法创建目录、读取或写入维度文件失败 |

`PROJECT-001` 是项目配置聚合诊断，会尽量一次报告多个独立配置问题。

## CFT 词法错误

| 诊断码 | 名称 | 含义 |
| --- | --- | --- |
| `CFT-LEX-001` | `UnexpectedCharacter` | 非法字符 |
| `CFT-LEX-002` | `InvalidStringEscape` | 非法字符串转义 |
| `CFT-LEX-003` | `UnterminatedString` | 字符串未闭合 |
| `CFT-LEX-004` | `InvalidIntLiteral` | 整数字面量非法或溢出 |
| `CFT-LEX-005` | `InvalidFloatLiteral` | 浮点字面量非法、溢出或非有限 |

## CFT 语法错误

| 诊断码 | 名称 | 含义 |
| --- | --- | --- |
| `CFT-SYN-001` | `UnexpectedToken` | 遇到不期望的 token |
| `CFT-SYN-002` | `UnexpectedEof` | 文件意外结束 |
| `CFT-SYN-003` | `ExpectedIdentifier` | 需要标识符 |
| `CFT-SYN-004` | `ExpectedToken` | 缺少固定 token，例如 `;` 或 `}` |
| `CFT-SYN-005` | `InvalidTopLevelItem` | 顶层只能出现 `const`、`enum` 或 `type` |
| `CFT-SYN-006` | `InvalidChainComparison` | 链式比较方向不一致，或链式比较中使用 `!=` |
| `CFT-SYN-007` | `CheckBlockMustBeLast` | `check` 块后又出现字段声明 |
| `CFT-SYN-008` | `InvalidAnnotationSyntax` | 注解语法非法 |
| `CFT-SYN-009` | `InvalidCheckStatement` | `check` 块内不是合法条件语句、量词块或 `when` 块 |
| `CFT-SYN-010` | `DuplicateCheckBlock` | 同一个 `type` 内声明多个 `check` 块 |

## CFT Schema 错误

| 诊断码 | 名称 | 含义 |
| --- | --- | --- |
| `CFT-SCHEMA-001` | `DuplicateModule` | 重复注册同一 module id |
| `CFT-SCHEMA-002` | `DuplicateGlobalName` | `const`、`enum`、`type` 全局重名 |
| `CFT-SCHEMA-003` | `DuplicateFieldName` | 同一 `type` 内字段重名 |
| `CFT-SCHEMA-004` | `DuplicateEnumVariant` | 同一 `enum` 内变体名重名 |
| `CFT-SCHEMA-005` | `DuplicateEnumValue` | 同一 `enum` 内整数值重名 |
| `CFT-SCHEMA-006` | `UnknownNamedType` | 字段类型引用未知 type 或 enum |
| `CFT-SCHEMA-007` | `ParentMustBeType` | 父类引用的名称不是 type |
| `CFT-SCHEMA-008` | `UnknownConst` | 默认值引用未知 const |
| `CFT-SCHEMA-009` | `InheritanceCycle` | 继承循环 |
| `CFT-SCHEMA-010` | `InheritSealedType` | 继承 `sealed type` |
| `CFT-SCHEMA-011` | `DuplicateInheritedField` | 子类声明了父类任意层级已有字段 |
| `CFT-SCHEMA-012` | `ConflictingTypeModifiers` | `abstract` 和 `sealed` 同时使用 |
| `CFT-SCHEMA-014` | `InvalidDictKeyType` | 字典 key 不是 `string`、`int` 或 enum |
| `CFT-SCHEMA-015` | `InvalidDefaultExpression` | 默认值不是编译期常量 |
| `CFT-SCHEMA-016` | `DefaultTypeMismatch` | 默认值类型与字段类型不匹配 |
| `CFT-SCHEMA-017` | `DefaultReferencesField` | 默认值引用字段或对象运行期值 |
| `CFT-SCHEMA-018` | `InvalidEnumValueSequence` | 枚举自动编号溢出或无法继续编号 |
| `CFT-SCHEMA-019` | `InvalidFlagEnumValue` | `@flag` enum 变体值不是 2 的幂 |
| `CFT-SCHEMA-020` | `UnknownAnnotation` | 未知注解名称 |
| `CFT-SCHEMA-021` | `DuplicateAnnotation` | 同一目标重复使用不允许重复的注解 |
| `CFT-SCHEMA-022` | `AnnotationWithoutTarget` | 注解后没有可附加目标 |
| `CFT-SCHEMA-023` | `InvalidAnnotationTarget` | 注解用在不支持的目标上 |
| `CFT-SCHEMA-024` | `InvalidAnnotationArgument` | 注解参数数量或类型错误 |
| `CFT-SCHEMA-025` | `InvalidAnnotatedFieldType` | `@expand`、`@ref` 或 `@inline` 字段类型或组合不合法 |
| `CFT-SCHEMA-026` | `StructRequiresSealedType` | `@struct` 标注的 type 不是 `sealed type` |
| `CFT-SCHEMA-027` | `IdAsEnumRequiresEmptyEnum` | `@idAsEnum` 参数不是已声明的空 enum |
| `CFT-SCHEMA-028` | `EnumVariantOnNonEnum` | 默认值使用 `Name.Variant`，但 `Name` 不是 enum |
| `CFT-SCHEMA-029` | `UnknownEnumVariant` | 默认值引用未知 enum variant |
| `CFT-SCHEMA-030` | `InvalidConstValue` | `const` 值不是允许的字面量类型 |
| `CFT-SCHEMA-031` | `ReservedIdentifier` | 使用保留名命名 const、enum、type、字段、enum variant 或量词变量 |
| `CFT-SCHEMA-034` | `LocalizedOnInvalidTarget` | `@localized` 用在不支持的目标上 |
| `CFT-SCHEMA-035` | `LocalizedBucketNotIdentifier` | `@localized("...")` 参数不是合法 CFT 标识符 |
| `CFT-SCHEMA-036` | `SingletonOnAbstractType` | `@singleton` 用在 `abstract type` 上 |
| `CFT-SCHEMA-037` | `SingletonIdAsEnumConflict` | `@singleton` 与 `@idAsEnum` 同时使用 |
| `CFT-SCHEMA-038` | `SingletonNotReferenceable` | `@singleton` type 被作为字段类型引用 |

## CFT Check 类型错误

| 诊断码 | 名称 | 含义 |
| --- | --- | --- |
| `CFT-TYPE-001` | `UnknownValueName` | `check` 表达式引用未知字段、量词变量、const 或 enum 名称 |
| `CFT-TYPE-002` | `UnknownField` | 字段访问的目标类型中不存在该字段 |
| `CFT-TYPE-003` | `UnknownEnumVariant` | `check` 表达式引用未知 enum variant |
| `CFT-TYPE-004` | `EnumVariantOnNonEnum` | `check` 表达式使用 `Name.Variant`，但 `Name` 不是 enum |
| `CFT-TYPE-005` | `OperatorTypeMismatch` | 运算符不支持操作数类型 |
| `CFT-TYPE-006` | `ComparisonTypeMismatch` | 不可比较类型，例如 enum 与 int |
| `CFT-TYPE-007` | `ConditionMustBeBool` | `check` 条件、`when` 条件或量词块条件结果不是 bool |
| `CFT-TYPE-008` | `UnknownFunction` | 未知内建函数 |
| `CFT-TYPE-009` | `FunctionArityMismatch` | 函数参数数量错误 |
| `CFT-TYPE-010` | `FunctionArgTypeMismatch` | 函数参数类型错误 |
| `CFT-TYPE-011` | `FieldAccessOnNonObject` | 对非对象做字段访问 |
| `CFT-TYPE-012` | `IndexOnNonIndexable` | 对非 array/dict 做索引访问 |
| `CFT-TYPE-013` | `IndexTypeMismatch` | array index 不是 int，或 dict key 类型不匹配 |
| `CFT-TYPE-014` | `InvalidIsPredicate` | `is` 目标不是 type 或 null |
| `CFT-TYPE-015` | `QuantifierRequiresCollection` | `all`、`any`、`none` 的目标不是 array/dict |
| `CFT-TYPE-016` | `UniqueUnsupportedElementType` | `unique` 的元素类型不支持 |
| `CFT-TYPE-017` | `BitwiseRequiresIntOrFlagEnum` | 位运算类型非法 |
| `CFT-TYPE-018` | `ShiftRequiresInt` | `<<`、`>>` 操作数不是 int |
| `CFT-TYPE-019` | `RegexPatternMustBeLiteral` | `matches` 的 pattern 不是字符串字面量 |
| `CFT-TYPE-020` | `InvalidRegexPattern` | `matches` 的正则 pattern 无法编译 |

## 表格与单元格

### Excel

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `EXCEL-SOURCE` | `EXCEL` | Excel source resolve/preflight 失败，例如扩展名不支持、路径或 options 不合法 |
| `EXCEL-OPEN` | `EXCEL` | workbook 无法打开 |
| `EXCEL-SHEET` | `EXCEL` | sheet 不存在、为空或无法读取 |
| `EXCEL-TYPE` | `EXCEL` | sheet 映射到未知 CFT type |
| `EXCEL-COLUMN` | `EXCEL` | 表头映射错误，例如未知字段、重复字段、`@expand` 相邻列不合法 |
| `EXCEL-ID` | `EXCEL` | key 列缺失、key 为空或 key 非法 |
| `EXCEL-CELL` | `EXCEL` | 不支持的 Excel 原生单元格值，例如 error、date/time、duration |
| `EXCEL-WRITE` | `EXCEL` | Excel writer 写回失败 |

### CSV

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `CSV-SOURCE` | `CSV` | CSV source resolve/preflight/load 失败，例如扩展名、路径、表头或 options 不合法 |
| `CSV-READ` | `CSV` | CSV 文件读取失败 |
| `CSV-PARSE` | `CSV` | CSV 内容解析失败 |
| `CSV-SHEET` | `CSV` | CSV 表格映射失败 |
| `CSV-TYPE` | `CSV` | CSV source 映射到未知 CFT type |
| `CSV-COLUMN` | `CSV` | 表头映射错误，例如未知字段、重复字段、`@expand` 相邻列不合法 |
| `CSV-ID` | `CSV` | key 列缺失、key 为空或 key 非法 |
| `CSV-WRITE` | `CSV` | CSV writer 写回失败 |

### 单元格值

`CELL-*` 来自 schema-guided cell parser。

| 诊断码 | 名称 | 含义 |
| --- | --- | --- |
| `CELL-Syntax` | `Syntax` | 单元格文本语法错误 |
| `CELL-InvalidDeclaredType` | `InvalidDeclaredType` | 传入 parser 的目标类型文本非法 |
| `CELL-UnknownType` | `UnknownType` | 单元格目标类型引用未知 CFT 类型 |
| `CELL-UnknownField` | `UnknownField` | 对象字段名不存在 |
| `CELL-DuplicateField` | `DuplicateField` | 对象内重复填写同一字段 |
| `CELL-MissingBoundary` | `MissingBoundary` | 嵌套数组、对象或字典缺少必要边界 |
| `CELL-TypeMismatch` | `TypeMismatch` | 值不能按目标类型解析 |
| `CELL-ObjectTypeMismatch` | `ObjectTypeMismatch` | 多态对象实际类型不能赋给目标类型 |
| `CELL-AbstractObjectType` | `AbstractObjectType` | 直接实例化 abstract type |
| `CELL-InvalidEnumVariant` | `InvalidEnumVariant` | enum variant 不存在 |
| `CELL-MixedObjectStyle` | `MixedObjectStyle` | 同一对象混用位置写法和字段名写法 |
| `CELL-StringNeedsQuotes` | `StringNeedsQuotes` | 字符串内容需要加引号 |
| `CELL-ReferenceNeedsMarker` | `ReferenceNeedsMarker` | 对象引用缺少 `@Type.key` 或 `&key` 标记 |

## CFD 文本与写回

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `CFD-SOURCE` | `CFD` | CFD source resolve/preflight 失败，例如扩展名或路径不合法 |
| `CFD-READ` | `CFD` | CFD 文件读取失败 |
| `CFD-WRITE` | `CFD` | CFD writer 写回失败 |
| `CFD-TEXT-Syntax` | `CFD` | CFD 文本语法错误 |
| `CFD-TEXT-UnknownType` | `CFD` | 文本记录或对象使用未知类型 |
| `CFD-TEXT-UnknownField` | `CFD` | 字段名不存在 |
| `CFD-TEXT-ObjectTypeMismatch` | `CFD` | 多态对象实际类型不能赋给目标类型 |
| `CFD-TEXT-MissingObjectType` | `CFD` | 需要具体对象类型但未提供 |
| `CFD-TEXT-TypeMismatch` | `CFD` | 值不能按目标类型解析 |
| `CFD-TEXT-InvalidEnumVariant` | `CFD` | enum variant 不存在 |
| `CFD-TEXT-ReferenceNeedsMarker` | `CFD` | 对象引用缺少 `@Type.key` 或 `&key` 标记 |

## 飞书/Lark

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `LARK-SOURCE` | `LARK` | Lark source resolve/preflight 失败 |
| `LARK-URL` | `LARK` | Lark URL 或 token 无法解析 |
| `LARK-AUTH` | `LARK` | tenant token 或鉴权失败 |
| `LARK-WIKI` | `LARK` | wiki 节点解析或读取失败 |
| `LARK-SHEET` | `LARK` | spreadsheet / sheet 元数据读取失败 |
| `LARK-VALUE` | `LARK` | 表格值读取或响应解析失败 |
| `LARK-WRITE` | `LARK` | Lark writer 写回失败 |

Lark 表格加载后会复用共享表格解析规则。表头、key、column 和 cell 语义与 Excel/CSV 一致；远端 API、鉴权、URL 和 wiki 解析问题使用 `LARK-*` 诊断。

## DataModel

| 诊断码 | 名称 | 含义 |
| --- | --- | --- |
| `CFD-DATA-001` | `UnknownType` | 未知 record 或 object type |
| `CFD-DATA-002` | `AbstractRecordType` | 顶层记录直接使用 abstract type |
| `CFD-DATA-003` | `MissingObjectType` | 多态 object 缺少实际类型 |
| `CFD-DATA-004` | `ObjectTypeMismatch` | object actual type 不可赋给声明类型 |
| `CFD-DATA-005` | `UnknownField` | 未知字段 |
| `CFD-DATA-006` | `MissingRequiredField` | 缺少必填字段 |
| `CFD-DATA-007` | `TypeMismatch` | value 类型不匹配 |
| `CFD-DATA-008` | `InvalidEnumVariant` | enum variant 非法 |
| `CFD-DATA-009` | `DuplicateDictKey` | dict key 重复 |
| `CFD-DATA-010` | `MissingIdField` | 缺少 ID 字段 |
| `CFD-DATA-011` | `DuplicateId` | 同一 type 内 record key 重复 |
| `CFD-DATA-012` | `DuplicatePolymorphicId` | polymorphic range 内 record key 重复 |
| `CFD-DATA-013` | `InvalidRecordKey` | record key identifier 非法 |
| `CFD-DATA-015` | `SingletonRecordCountInvalid` | `@singleton` type 的 records 数量不等于 1 |
| `CFD-DATA-016` | `SingletonKeyMissingOrInvalid` | `@singleton` type 的 record key 缺失或非法 |
| `CFD-DATA-017` | `SingletonKeyCollision` | 不同 `@singleton` type 的 record key 撞名 |

## 引用解析

| 诊断码 | 名称 | 含义 |
| --- | --- | --- |
| `CFD-REF-001` | `RefTargetNotFound` | 找不到被引用的目标记录 |

路径字段、数组索引或字典 key 不合法时，也会在引用解析阶段报告。

## Check 运行期

| 诊断码 | 名称 | 含义 |
| --- | --- | --- |
| `CFD-CHECK-001` | `CheckFailed` | 兜底 false 条件 |
| `CFD-CHECK-002` | `CheckEvalTypeError` | check 求值运行期类型错误 |
| `CFD-CHECK-003` | `CheckNullAccess` | 访问 null |
| `CFD-CHECK-004` | `CheckIndexOutOfBounds` | 数组索引越界 |
| `CFD-CHECK-005` | `CheckMissingDictKey` | 字典 key 不存在 |
| `CFD-CHECK-006` | `CheckEmptyMinMax` | `min` / `max` 没有非 null 值 |
| `CFD-CHECK-007` | `CheckComparisonFailed` | 比较条件失败 |
| `CFD-CHECK-008` | `CheckBoolExpectedTrue` | 裸 bool 表达式为 false |
| `CFD-CHECK-009` | `CheckNegationFailed` | `!expr` 失败 |
| `CFD-CHECK-010` | `CheckAndFailed` | `lhs && rhs` 失败 |
| `CFD-CHECK-011` | `CheckOrFailed` | `lhs || rhs` 失败 |
| `CFD-CHECK-012` | `CheckTypePredicateFailed` | `expr is TypeName` 失败 |
| `CFD-CHECK-013` | `CheckNullPredicateFailed` | null 谓词失败 |
| `CFD-CHECK-014` | `CheckContainsFailed` | `contains` 返回 false |
| `CFD-CHECK-015` | `CheckUniqueFailed` | `unique` 返回 false |
| `CFD-CHECK-016` | `CheckMatchesFailed` | `matches` 返回 false |
| `CFD-CHECK-017` | `CheckAnyQuantifierFailed` | `any` 没有元素满足 |
| `CFD-CHECK-018` | `CheckNoneQuantifierFailed` | `none` 存在满足条件的元素 |
| `CFD-CHECK-019` | `CheckAllQuantifierFailed` | `all` 存在不满足条件的元素 |

`when` 不分配独立错误码。`when` 条件为 true 且 body 内规则失败时，诊断使用内部真实规则的错误码。

## Codegen 与产物

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `CODEGEN-CSHARP-001` | `CODEGEN` | C# codegen preflight 诊断，例如 namespace、类型名、成员名、文件名或 `@idAsEnum` variant 非法 |
| `CSHARP-CODEGEN` | `CODEGEN` | C# codegen 生成阶段错误 |
| `CODEGEN` | `CODEGEN` | codegen provider 通用诊断 |
| `ARTIFACT-001` | `ARTIFACT` | 输出目录、artifact path、staging、commit 或 lockfile 读写/解析失败 |

存在 `CODEGEN-CSHARP-001` 时，Coflow 不读写 enum lockfile，不替换 C# 输出目录，也不生成新的 `.cs` 文件。

## 通用写回

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `WRITE-UNSUPPORTED` | `WRITE` | 当前 writer 不支持请求的写入能力，例如插入、删除或重命名记录 |

具体 provider 写回失败通常会使用 provider 自己的诊断码，例如 `EXCEL-WRITE`、`CSV-WRITE`、`CFD-WRITE` 或 `LARK-WRITE`。

## 非阻塞收集规则

“非阻塞”表示 Coflow 会在不依赖无效中间数据的前提下继续收集诊断；不表示会在输入无效时生成产物。

| 阶段 | 收集行为 |
| --- | --- |
| 项目配置 | 独立字段会尽量一起报告 |
| CFT lex/syntax | 单文件内有限恢复，其他 schema 文件仍可继续 |
| CFT schema/type | 会尽量聚合 |
| Excel workbook/sheet | 其他 workbook/sheet 可继续 |
| Excel 表头 | 表头错误会跳过该 sheet 的数据行 |
| Cell parse | 有效表头下跨行收集 |
| CFD 文本 | 文件间继续，单文件内有限恢复 |
| DataModel | 子阶段内聚合；无效 model 阻塞引用和 check |
| 引用解析 | 子阶段内聚合；未解析引用阻塞 check |
| Check | 跨 block 和 record 聚合；硬运行期错误只停止当前 block |
| Codegen / artifact preflight | 尽量聚合 |

## 修复顺序

建议按阶段顺序修：

1. `CLI` / `PROJECT` / `DIM-CONFIG`：命令和项目配置。
2. `CFT-LEX` / `CFT-SYN` / `CFT-SCHEMA` / `CFT-TYPE`：CFT schema。
3. `EXCEL` / `CSV` / `LARK` / `CELL` / `CFD-TEXT`：数据源读取和文本解析。
4. `CFD-DATA` / `CFD-REF`：DataModel 和引用。
5. `CFD-CHECK`：业务规则。
6. `CODEGEN` / `ARTIFACT`：生成和输出目录。

前序阶段出错时，后续阶段可能不会运行。例如 DataModel 无效时，引用解析和 check 不会继续。
