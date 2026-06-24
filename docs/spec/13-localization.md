# 本地化规格

**依赖文档**：[01-cft.md](01-cft.md)、[02-data-model.md](02-data-model.md)、[02-schema-api.md](02-schema-api.md)、[06-csharp-codegen.md](06-csharp-codegen.md)、[07-project-pipeline.md](07-project-pipeline.md)

本地化层定义 `@localized` 注解的语义、翻译表 key 生成规则、CSV 翻译表的产物结构与增量更新策略，以及多语言 `check` 执行行为。CFT 语言本身只承载注解，不负责加载翻译表；翻译表的生成与合并由 `coflow-engine` 的 localization 模块完成。

---

## 目录

1. [设计目标与职责边界](#1-设计目标与职责边界)
2. [`@localized` 注解](#2-localized-注解)
3. [翻译 key 生成规则](#3-翻译-key-生成规则)
4. [CSV 翻译表结构](#4-csv-翻译表结构)
5. [增量更新策略](#5-增量更新策略)
6. [多语言 check 执行](#6-多语言-check-执行)
7. [`coflow.yaml` 配置](#7-coflowyaml-配置)
8. [C# 运行时形态](#8-c-运行时形态)
9. [错误码](#9-错误码)

---

## 1. 设计目标与职责边界

`@localized` 解决"同一字段在不同语言下取不同值"的需求。设计目标：

- 作者只在数据源里写默认语言内容，不手写翻译 key
- 翻译表 key 由编译器自动生成，稳定、可复现
- 默认表由工具链权威生成；其它语言列由人工/翻译流程维护，工具链不覆盖
- 多语言 check 在编译期一并校验，避免某种语言下数据违反约束被遗漏到运行期

CFT 层只声明哪些字段是本地化的；翻译表的解析、合并、消费由 engine 与运行时分别承担。

---

## 2. `@localized` 注解

### 2.1 适用范围

| 项 | 规则 |
|----|------|
| 目标 | `type` 内字段 |
| 字段类型 | 任意（primitive、`[T]`、`{K:V}`、嵌套对象、`T?` 均可） |
| 不可用于 | `const`、`enum`、`enum variant`、`type` 本身 |

### 2.2 形式与参数

无参形式：

```cft
@localized
name: string;
```

带 bucket 参数：

```cft
@localized("story")
description: string;
```

参数为单个字符串字面量，必须是合法 CFT 标识符（Unicode XID）。

### 2.3 分表（bucket）

- 不带参数时，bucket 名 = 字段所在 type 名
- 带参数时，bucket 名 = 参数值
- 不需要在 `coflow.yaml` 预先声明 bucket 列表；任何合法标识符都可作为 bucket，编译器按实际出现集合输出对应 CSV 文件

### 2.4 字段层级语义

`@localized` 把**整个字段值**作为本地化单位，不下钻到数组元素或 dict value。即：

```cft
@localized
tags: [string];          # 整个 [string] 数组作为一条翻译条目
```

这条翻译条目在 CSV 单元格里以"完整值"形式存放（编码方式见 §4.3）。

`@localized` 出现在嵌套对象字段上时，仅该字段路径产生一条 key；嵌套对象内部的子字段不会单独生成 key（即使子字段也写了 `@localized`，那是另一个独立的 key）。

---

## 3. 翻译 key 生成规则

### 3.1 格式

```
{Bucket}/{record_key}/{field_path}
```

`{field_path}` 由"嵌套对象字段名"以 `/` 拼接而成。CFT 数据模型中字段访问只跨"嵌套对象 → 子字段"边界；本规则不下钻数组/dict 元素，因此 `field_path` 永远是合法标识符序列。

### 3.2 合法性约束

- `Bucket`、`TypeName`、字段名段已由 CFT 标识符规则保证为合法 XID 标识符
- `record_key` 必须满足 CFT 标识符规则（Unicode XID）；不满足时报 `CFD-DATA-014 LocalizedRecordKeyInvalid`，在 `CfdDataModel` build 阶段判定
- 路径段之间使用 `/` 分隔；`/` 不属于 XID 字符，不会与任何标识符冲突

### 3.3 示例

```
Item/potion/name
Item/potion/description
Skill/fireball/tooltip
story/main_quest/intro
```

---

## 4. CSV 翻译表结构

### 4.1 文件布局

```
<localization_out_dir>/
  Item.csv
  Skill.csv
  story.csv
  ...
```

每个 bucket 一个 CSV 文件。文件名 = bucket 名 + `.csv`。

### 4.2 列定义

| 列序 | 列名 | 含义 |
|------|------|------|
| 1 | `key` | 翻译 key |
| 2 | `default` | 数据源里写入的字面值（默认语言、运行期 fallback 来源） |
| 3..N | 各语言代号 | 由 `coflow.yaml` `localization.languages` 声明的语言 |

CSV 第一行为表头，后续行按 key 字典序写出。

### 4.3 单元格值序列化

CSV 单元格采用一种**简单稳定的内部编码**，由工具链权威生成。设计原则：可读、易于人工核对，但不承诺与 Excel loader 的 cell parser 语法严格兼容；译者通常只编辑 string 字段对应的单元格，复合类型仅作展示。

| 字段类型 | 单元格内容 |
|----------|-----------|
| string | 原文 |
| int / float / bool | 字面量 |
| null（nullable 字段值为 null） | 空单元格 |
| enum | `EnumName.Variant`（命名变体）或 `EnumName(N)`（数值） |
| `[T]` | `[a, b, c]` |
| `{K:V}` | `{key1: value1, key2: value2}`（string key 加双引号，int/enum key 直接写） |
| 嵌套对象 | `{field1: value1, field2: value2}` |
| record 引用 | `&key` |

> CSV 重新加载时，工具链按"原样"读取该单元格作为字符串供运行时使用；不会反向解析回结构化值。需要结构化覆盖请通过更上层的翻译流水线处理。

CSV 自身的转义遵循 RFC 4180：含 `,`、`"`、换行的字段加双引号，内部 `"` 写作 `""`。

---

## 5. 增量更新策略

`coflow-engine` localization 模块在 build 阶段执行下列步骤：

1. 扫描 `CfdDataModel`，按 (bucket, record_key, field_path) 收集 `(key, default_value)` 条目
2. 对每个 bucket 生成内存表：`key, default, <lang>...`，`default` 列填本次扫描值，其它语言列暂为空
3. 读取磁盘上同名 CSV（如存在），把已有的非 default 语言列值合并进内存表（按 key 匹配；忽略数据源已不存在的旧 key —— 见下文删行规则）
4. 写回磁盘

### 5.1 列写入策略

| 列 | 行为 |
|----|------|
| `key` | 由工具链权威决定 |
| `default` | 每次 build 整体重写 |
| 其它语言列 | 仅当磁盘上原值非空时保留；新增 key 时该列留空 |

工具链**永远不写入**非 default 语言列的非空值，除非该 key 的该语言列原本就是空（首次出现）。

### 5.2 废弃 key 处理

数据源中不再产生的 key（人工/翻译表里仍存在）→ **直接删除整行**，不保留废弃区。

### 5.3 语言列增删

- 在 `coflow.yaml` 新增语言 → 该语言列被加入所有 CSV，所有行该列为空
- 删除语言 → 该列从 CSV 中移除（已写入的翻译数据丢失，删除前请人工备份）

### 5.4 列序

CSV 列序固定为 `key, default, <coflow.yaml 中 languages 的声明顺序>`。`coflow.yaml` 中调整 `languages` 顺序会改变 CSV 列序；不影响数据正确性。

---

## 6. 多语言 check 执行

### 6.1 默认轮

`coflow-checker` 仍按现有规则跑一轮 check，使用数据源原始字面值。诊断标签为 `language=default`。

### 6.2 各语言轮

对 `coflow.yaml` 中声明的每种语言，再跑一轮 check：

- 访问被 `@localized` 标记的字段时，取该语言对应的翻译表值
- 若该语言对应 key 缺失（CSV 单元格为空）→ fallback 到 `default` 列值
- 其它字段（非 `@localized`）取数据源原始值
- 诊断标签为 `language=<lang_code>`

### 6.3 诊断聚合

任一语言轮的失败都被记录，互不短路。诊断输出按 (语言, record, path) 排序。

### 6.4 性能与跳过

多语言轮线性放大 check 时间。允许 CLI 通过 `--skip-localized-check` 跳过非 default 轮（仅默认语言 check）。该开关只影响开发期循环速度，不改变语义。

---

## 7. `coflow.yaml` 配置

```yaml
localization:
  out_dir: "data/localization"     # 翻译表输出目录，相对项目根
  languages:                       # 项目支持的非 default 语言列表
    - "zh_CN"
    - "en"
    - "ja"
```

### 7.1 字段语义

| 字段 | 类型 | 必填 | 默认 | 说明 |
|------|------|------|------|------|
| `out_dir` | string | 否 | `data/localization` | 翻译表输出目录 |
| `languages` | string[] | 否 | `[]` | 语言代号列表，必须为合法 CFT 标识符 |

### 7.2 缺省行为

`coflow.yaml` 中**完全不写** `localization` 段时：

- 不生成翻译表文件
- check 只跑默认轮
- C# codegen 仍把 `@localized` 字段渲染为 `Localized<T>`（见 §8）

这意味着 schema 中即使写了 `@localized`，只要 yaml 不开启，行为就退化为"标记字段类型但不维护翻译表"。

### 7.3 `languages` 命名

- 每个语言代号必须是合法 CFT 标识符（Unicode XID），如 `zh_CN`、`en`、`ja`、`fr_CA`
- 不允许 `default` 作为语言代号（与 `default` 列冲突），违反时报 `CFG-LOC-001 ReservedLanguageCode`
- 列表内不允许重复，违反时报 `CFG-LOC-002 DuplicateLanguageCode`

---

## 8. C# 运行时形态

被 `@localized` 标记的字段在 C# codegen 中统一包装为 `Localized<T>`，`T` 为字段原始 CLR 类型。详见 [06-csharp-codegen.md](06-csharp-codegen.md)。

### 8.1 包装类型契约

```csharp
public readonly struct Localized<T> {
    public string Key { get; }                  // 形如 "Item/potion/name"
    public T Default { get; }                   // 数据源里写入的默认值
    public T Value => Localization.Current.Get<T>(Key, Default);
    public T For(string lang) => Localization.For(lang).Get<T>(Key, Default);
    public static implicit operator T(Localized<T> s) => s.Value;
}
```

### 8.2 运行时职责（宿主侧）

`Localization.Current` / `Localization.For(lang)` 是宿主侧接口，CFT 工具链不规定其具体实现。建议契约：

- 加载 CSV / 已编译翻译表
- 按当前语言查 key，缺失返回 `null`
- `Localized<T>.Value` 在缺失时使用 `Default`

C# codegen 会随生成代码附带一份默认 `Localization` 实现作为起点（一次性生成到 `Coflow.Runtime/Localization.cs`），宿主可按需替换。

---

## 9. 错误码

### 9.1 CFT 阶段（schema 层）

| 错误码 | 名称 | 含义 |
|--------|------|------|
| `CFT-SCHEMA-034` | `LocalizedOnInvalidTarget` | `@localized` 用在 const、enum、enum variant 或 type 本身上 |
| `CFT-SCHEMA-035` | `LocalizedBucketNotIdentifier` | `@localized("...")` 参数不是合法 CFT 标识符 |

### 9.2 数据模型阶段

| 错误码 | 名称 | 含义 |
|--------|------|------|
| `CFD-DATA-013` | `InvalidRecordKey` | 含 `@localized` 字段的 record 其 key 不是合法 CFT 标识符（与普通 record key 共用此码） |

### 9.3 项目配置阶段

| 错误码 | 名称 | 含义 |
|--------|------|------|
| `CFG-LOC-001` | `ReservedLanguageCode` | `languages` 中包含 `default` |
| `CFG-LOC-002` | `DuplicateLanguageCode` | `languages` 中存在重复语言代号 |
| `CFG-LOC-003` | `InvalidLanguageCode` | 语言代号不是合法 CFT 标识符 |

### 9.4 翻译表 IO 阶段

| 错误码 | 名称 | 含义 |
|--------|------|------|
| `LOC-IO-001` | `TranslationParseError` | CSV 解析失败 |
| `LOC-IO-002` | `TranslationCellInvalid` | 翻译单元格值无法解析为字段类型 |
| `LOC-IO-003` | `TranslationFileWriteFailed` | 翻译表写盘失败 |
