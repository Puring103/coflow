# 维度与本地化

Coflow 的维度/变体机制用于表达“同一个字段在不同上下文下取不同值”。本地化是当前内建的维度场景：`language` 维度表示语言，`zh`、`en`、`ja` 等是语言变体。

## 维度与变体

维度（dimension）是一条变化轴。变体（variant）是这条轴上的具体取值。

```text
dimension: language
  variants:
    default
    zh
    en
    ja
```

`default` 是源数据中的默认值，不写在 `variants` 配置里。`variants` 只声明额外变体。

维度字段是被某个维度切分的字段。读取和校验这个字段时，Coflow 可以在默认值和某个变体值之间切换。

## 配置维度

当前内建维度是 `language`。在 `coflow.yaml` 中使用 `dimensions.language` 启用：

```yaml
dimensions:
  language:
    variants:
      - zh
      - en
      - ja
    out_dir: data/dimensions/language
    display_name: 本地化
```

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `variants` | 是 | 维度变体列表，不包含 `default` |
| `out_dir` | 是 | 维度数据文件目录 |
| `display_name` | 否 | 编辑器展示名称 |

`variants` 中的每个值必须是合法 CFT 标识符，不能重复，不能是保留变体 `default`。

## 维度字段

维度字段通过 CFT 注解声明。当前可用注解是 `@localized`，它把字段归入 `language` 维度：

```text
type Item {
  @localized
  name: string;

  @localized
  description: string;
}
```

含义是：

- 源数据中的 `name` / `description` 是默认值。
- `zh`、`en`、`ja` 等变体值维护在 `dimensions.language.out_dir` 下的维度文件中。
- 检查时，Coflow 会用默认值和各语言变体分别执行相关规则。

如果 schema 中存在 `@localized` 字段，但没有配置 `dimensions.language`，Coflow 会报告项目配置诊断。

## 合成 Type

每个维度字段会在内存中生成一个合成 type，用来表示默认值和各变体值。

例如：

```text
type Item {
  @localized
  name: string;
}
```

在 `variants: [zh, en]` 下，概念上会生成：

```text
type Item_nameVariants {
  default: string?;
  zh: string?;
  en: string?;
}
```

合成 type 不需要写进用户的 `.cft` 文件。它由 engine 注入，参与普通数据加载、索引和 check。

## 维度文件

普通 type 的维度字段生成 CSV：

```text
data/dimensions/language/
  Item_name.csv
  Item_description.csv
```

CSV 结构：

```csv
id,default,zh,en
sword_fire,Fire Sword,火焰剑,Fire Sword
staff_ice,Ice Staff,冰杖,Ice Staff
```

| 列 | 说明 |
| --- | --- |
| `id` | 源 record key |
| `default` | 源数据中的默认值，由 Coflow 刷新 |
| 变体列 | 对应 variant 的值，由人工或翻译流程维护 |

singleton type 的维度字段生成 CFD 文件，每个字段一条 record。

维度文件作为隐式 source 加载，不需要手动写进 `sources`。

## 生成与保留策略

构建项目时，Coflow 会维护维度文件：

- 新的维度字段会生成对应文件。
- 新的源 record 会生成对应行。
- `default` 列会根据源数据刷新。
- 已存在的变体列值会保留。
- 删除源 record 或字段后，对应维度数据不再参与当前项目模型。

维度 CSV 使用普通表格数据源语义。`default` 和变体列里的值按合成 type 字段类型解析，因此可以使用 [单元格值语法](./03-language/03-cell-value.md)：

```csv
id,default,zh,en
sword,weapon | melee,武器 | 近战,weapon | melee
```

非字符串字段也可以进入维度流程，但变体值必须符合原字段类型。

## 本地化作为语言维度

本地化就是 `language` 维度的一种使用方式。`@localized` 声明字段随语言变化：

```text
type Item {
  @localized
  name: string;
}
```

也可以指定 bucket：

```text
type Item {
  @localized("ui")
  icon_text: string;
}
```

规则：

- `@localized` 只能写在 type 字段上。
- 字段类型可以是 primitive、enum、数组、字典、对象或 nullable。
- `@localized` 作用于整个字段值，不下钻到数组元素或字典 value。
- 不能用于 `const`、`enum`、enum variant 或 type 本身。
- bucket 必须是合法 CFT 标识符。

## Check 行为

`check {}` 会在默认值轮和语言变体轮中执行。

默认值轮使用源数据中的字段值。语言变体轮访问 `@localized` 字段时，会尝试读取对应语言列：

- 语言列有值：使用该值。
- 语言列为空或为 `null`：该字段在该语言轮不替换，按维度求值规则处理。
- 非维度字段：始终使用源数据值。

语言轮产生的诊断会带上语言上下文，例如 `[language=zh]`。

## C# 运行时

C# codegen 会把 `@localized` 字段包装为 `Localized<T>`：

```csharp
public Localized<string> Name { get; }
```

使用方式：

```csharp
var defaultName = item.Name.Default;
var currentName = item.Name.Value;
var englishName = item.Name.For("en");
```

`Localized<T>` 保存 key 和默认值。宿主可以替换或扩展生成的 `Localization` helper，以接入自己的语言切换和翻译表加载方式。

## 常见错误

| 问题 | 原因 | 处理 |
| --- | --- | --- |
| `@localized` 写在 type 上 | 注解目标非法 | 写在字段上 |
| bucket 写成 `bad-name` | bucket 不是合法 CFT 标识符 | 使用 `bad_name` |
| 配了 `@localized` 但没有 `dimensions.language` | 语言维度未启用 | 在 `coflow.yaml` 添加 `dimensions.language` |
| `variants` 为空 | 没有任何语言变体 | 至少配置一个 variant |
| variant 写成 `zh-CN` | `-` 不是 CFT 标识符字符 | 写 `zh_CN` |
| 变体列值类型错误 | 维度字段仍按原字段类型解析 | 按单元格值语法修正 |
