# 错误码索引

本页列出 Coflow 当前公开诊断码。诊断输出格式、阶段含义和收集规则见 [诊断](./01-diagnostics.md)。

## 命令级与项目配置

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `CLI-ERROR` | `CLI` | 不可恢复的命令级错误，例如配置文件无法读取、YAML 无法解析、命令参数无法进入结构化检查 |
| `PROJECT-001` | `PROJECT` | 已解析项目配置或命令 preflight 诊断，例如路径不存在、source 配置非法、output 缺失或类型不兼容 |
| `DIM-CONFIG-001` | `PROJECT` | schema 中存在 `@localized` 字段，但未配置 `dimensions.language` |
| `DIM-CONFIG-002` | `PROJECT` | `dimensions.language.variants` 为空、包含 `default`、包含重复项或不是合法 CFT 标识符 |
| `DIM-CONFIG-003` | `PROJECT` | `dimensions.language.out_dir` 缺失 |
| `DIM-SOURCE-001` | `PROJECT` / engine | 维度文件生成或隐式 source 注册失败，例如无法创建目录、读取或写入维度文件失败 |
| `CFT-LSP` | `LSP` | Language Server 无法解析当前文档或项目上下文时返回的编辑器诊断 |

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
| `CFT-SCHEMA-025` | `InvalidAnnotatedFieldType` | `@expand` 字段类型、`&Type` 引用类型或字段注解组合不合法 |
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
| `CFT-TYPE-016` | `UniqueUnsupportedElementType` | `isUnique` 的元素类型不支持 |
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
| `CELL-ReferenceNeedsMarker` | `ReferenceNeedsMarker` | 对象引用缺少 `&key` 标记 |

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
| `CFD-TEXT-ReferenceNeedsMarker` | `CFD` | 对象引用缺少 `&key` 标记 |

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
| `DATA-NOT-FOUND` | data read | `data get` / `data list` 查询不到目标 record |
| `DATA-GET-LIMIT` | data read | `data get` 匹配记录超过默认安全上限，需要显式 `--limit` 或 `--all` |
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
| `CFD-CHECK-015` | `CheckUniqueFailed` | `isUnique` 返回 false |
| `CFD-CHECK-016` | `CheckMatchesFailed` | `matches` 返回 false |
| `CFD-CHECK-017` | `CheckAnyQuantifierFailed` | `any` 没有元素满足 |
| `CFD-CHECK-018` | `CheckNoneQuantifierFailed` | `none` 存在满足条件的元素 |
| `CFD-CHECK-019` | `CheckAllQuantifierFailed` | `all` 存在不满足条件的元素 |

`when` 不分配独立错误码。`when` 条件为 true 且 body 内规则失败时，诊断使用内部真实规则的错误码。

## Codegen 与产物

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `JSON-EXPORT` | `EXPORT` | JSON exporter 编码失败 |
| `MESSAGEPACK-EXPORT` | `EXPORT` | MessagePack exporter 编码失败 |
| `CODEGEN-CSHARP-001` | `CODEGEN` | C# codegen preflight 诊断，例如 namespace、类型名、成员名、文件名或 `@idAsEnum` variant 非法 |
| `CSHARP-FORMAT` | `CODEGEN` | C# codegen 不支持当前数据导出格式 |
| `CSHARP-OPTIONS` | `CODEGEN` | C# codegen provider option 解析失败 |
| `CSHARP-CODEGEN` | `CODEGEN` | C# codegen 生成阶段错误 |
| `CODEGEN` | `CODEGEN` | codegen provider 通用诊断 |
| `ARTIFACT-001` | `ARTIFACT` | 输出目录、artifact path、staging、commit 或 lockfile 读写/解析失败 |

存在 `CODEGEN-CSHARP-001` 时，Coflow 不读写 enum lockfile，不替换 C# 输出目录，也不生成新的 `.cs` 文件。

## 通用写回

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `WRITE-UNSUPPORTED` | `WRITE` | 当前 writer 不支持请求的写入能力，例如插入、删除或重命名记录 |
| `WRITE-NOT-FOUND` | `WRITE` | 写入目标 record 不存在 |
| `WRITE-NO-SOURCE` | `WRITE` | 找不到可承载写入请求的数据源 |
| `WRITE-NO-WRITER` | `WRITE` | 目标 source 没有注册对应 writer |
| `WRITE-REBUILD` | `WRITE` | 写入后重新加载或校验项目失败 |
| `WRITE-RENAME` | `WRITE` | record key 重命名失败，例如新 key 已存在或引用改写失败 |
| `WRITE-SPREAD-SOURCE` | `WRITE` | 写入涉及 spread 来源记录，但无法定位或更新来源 |

具体 provider 写回失败通常会使用 provider 自己的诊断码，例如 `EXCEL-WRITE`、`CSV-WRITE`、`CFD-WRITE` 或 `LARK-WRITE`。

## 本地数据文件维护

`data create-file` 和 `data sync-header` 使用 `DATA-FILE` 阶段诊断。

| 诊断码 | 阶段 | 含义 |
| --- | --- | --- |
| `DATA-FILE-IO` | `DATA-FILE` | 创建、读取或写入本地数据文件失败 |
| `DATA-FILE-MISSING` | `DATA-FILE` | 目标文件不存在 |
| `DATA-FILE-EXISTS` | `DATA-FILE` | 目标文件或 Excel sheet 已存在，不能覆盖 |
| `DATA-FILE-PROVIDER` | `DATA-FILE` | provider 无法推断或 provider id 不支持 |
| `DATA-FILE-SOURCE` | `DATA-FILE` | 表格创建目标无法匹配已配置 source，例如 Lark source 未配置或地址不一致 |
| `DATA-FILE-TYPE` | `DATA-FILE` | 缺少 `--type`、类型未知或类型不适合创建数据文件 |
| `DATA-FILE-PARSE` | `DATA-FILE` | 读取现有 CFD/CSV 文件时解析失败 |
| `DATA-FILE-EXCEL` | `DATA-FILE` | Excel workbook 或 sheet 操作失败 |
