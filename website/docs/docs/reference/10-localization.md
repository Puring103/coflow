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

所有维度使用相同的配置和校验流程。`language` 是 `@localized` 使用的内建维度名，在 `coflow.yaml` 中使用 `dimensions.language` 启用：

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

维度字段通过 CFT 注解声明。`@localized` 是 `language` 维度的便捷写法：

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

如果 schema 中存在 `@localized` 字段，但没有配置 `dimensions.language`，Coflow 会报告缺失维度 binding 的 CFT schema 诊断。

其他维度使用 `@dimension("name")`：

```text
type Item {
  @dimension("platform")
  price: int;
}
```

注解中的名称必须对应 `coflow.yaml` 中已声明的维度。一个字段不能同时使用 `@localized` 和 `@dimension`。

## 默认值与变体

普通数据源中的字段值是默认值，维度文件中的 `default` 列用于展示和同步这个默认值。它不会创建另一份独立配置。

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

singleton type 的同一维度字段会合并到一份 CFD 文件，每个字段一条 record。

维度文件由 Coflow 自动发现，不需要手动写进 `sources`。变体值仍按原字段类型解析。

## 生成与保留策略

构建项目时，Coflow 会维护维度文件：

- 新的维度字段会生成对应文件。
- 新的源 record 会生成对应行。
- `default` 列会根据源数据刷新。
- 已存在的变体列值会保留。
- 重命名源 record 时，同一 transaction 会重命名维度行并保留变体值。
- 删除源 record 时，同一 transaction 会删除对应维度行。
- 删除字段后，对应维度文件不再参与当前项目模型。

维度 CSV 使用普通表格数据源语义。变体列按原 schema 字段类型解析，因此可以使用 [单元格值语法](./03-language/03-cell-value.md)：

```csv
id,default,zh,en
sword,weapon | melee,武器 | 近战,weapon | melee
```

非字符串字段也可以进入维度流程，但变体值必须符合原字段类型。

## 构建导出

`build` 和 `export` 会为每个维度字段额外生成一张 `{声明类型}_{字段名}Variants` 表。JSON 使用 `.json`，MessagePack 使用 `.msgpack`。例如：

```text
Item_nameVariants.json
```

表中每条记录包含源 record key、默认值和配置顺序下的全部变体：

```json
[
  {
    "id": "sword_fire",
    "default": "Fire Sword",
    "zh": "火焰剑",
    "en": "Fire Sword"
  }
]
```

缺失或显式为 `null` 的变体导出为 `null`。继承该字段的子 type record 会进入声明类型对应的 Variants 表；singleton 字段使用字段名作为 `id`。

启用代码生成时，生成结果会包含对应 Variants 表的读取 API。具体类型和命名规则取决于所选代码生成目标。

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

默认值和每个已提供的语言变体都会按 CFT 类型规则执行相关 `check {}`。继承链上的规则会从父类型到子类型依次执行。

与维度字段无关的规则只需要执行一次；读取维度字段的规则会在对应变体上下文中执行。

语言变体轮读取 `@localized` 字段时：

- 变体值存在：使用该值，其他非维度字段仍读取源 record。
- 变体值缺失：保持 missing，不回退到默认值。
- 变体值解析为 `null`：本轮跳过依赖该字段的语句和方法调用，不回退到默认值。
- 整个对象、数组或字典字段被 `@localized` 标记时，变体值的完整子树会在逻辑字段路径上执行嵌套 type check。
- 只有嵌套对象内部的字段被 `@localized` 标记、而外层对象字段本身不是维度字段时，不会因此重复检查整个外层 record。

因此，不读取任何维度字段的规则只在默认值上下文执行一次；一个语言字段缺少变体值也不会自动改用默认值再次执行。

语言轮产生的诊断会带上语言上下文，例如 `[language=zh]`。

## 代码生成

启用代码生成时，维度字段会生成能够读取默认值、当前变体和指定变体的访问 API。具体 API 形态由代码生成目标决定，详见对应目标的代码生成文档。

## 常见错误

| 问题 | 原因 | 处理 |
| --- | --- | --- |
| `@localized` 写在 type 上 | 注解目标非法 | 写在字段上 |
| bucket 写成 `bad-name` | bucket 不是合法 CFT 标识符 | 使用 `bad_name` |
| 配了 `@localized` 但没有 `dimensions.language` | 语言维度未启用 | 在 `coflow.yaml` 添加 `dimensions.language` |
| `variants` 为空 | 没有任何语言变体 | 至少配置一个 variant |
| variant 写成 `zh-CN` | `-` 不是 CFT 标识符字符 | 写 `zh_CN` |
| 变体列值类型错误 | 维度字段仍按原字段类型解析 | 按单元格值语法修正 |
