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

如果 schema 中存在 `@localized` 字段，但没有配置 `dimensions.language`，Coflow 会报告缺失维度 binding 的 CFT schema 诊断。

## Record Overlay

维度是编译后 schema 的一等对象。字段直接保存 dimension binding，DataModel 把额外变体值附着到原 owner record：

```text
Item.potion.fields.name                    = "Potion"
Item.potion.dimension_fields.name.zh       = "药水"
Item.potion.dimension_fields.name.en       = "Potion"
```

不会生成合成 type、storage record、runtime module 或独立 dimension value store。Schema inspect、记录列表和关系图只展示用户声明的 type 和 record。

默认值只来自普通 source field；维度文件中的 `default` 是用于同步和人工核对的物理镜像，不会作为第二份 default 加载。

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

singleton type 的同一维度字段合并到一份 CFD 文件，每个字段一条 record；生成、加载和写入都以整份物理文件为事务边界。

维度文件由 runtime 自动发现，不需要手动写进 `sources`。Provider 按原 `CftField` 类型直接解析变体值，并把值与 CSV cell 或 CFD span origin 一起交给 owner record overlay。

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

Schema 编译时会为每个实际 type 建立 typed check schedule。默认值轮按父 type
到子 type 的顺序执行完整继承链上的 `check {}`，并继续检查普通嵌套对象。

每个语言变体只执行静态读取了 `@localized` 字段的 check 语句，而不会重复执行与
`language` 无关的语句。子 type record 会同时执行父 type 中与语言相关的语句；量词
绑定等局部名称会按词法作用域分析，不会因为与字段同名而被误判成维度读取。

语言变体轮读取 `@localized` 字段时：

- 变体值存在：使用该值，其他非维度字段仍读取源 record。
- 变体值缺失：保持 missing，不回退到默认值。
- 变体值解析为 `null`：本轮跳过依赖该字段的语句和方法调用，不回退到默认值。
- 整个对象、数组或字典字段被 `@localized` 标记时，变体值的完整子树会在逻辑字段路径上执行嵌套 type check。
- 只有嵌套对象内部的字段被 `@localized` 标记、而外层对象字段本身不是维度字段时，不会因此为外层 record 启动额外语言轮。

因此，不读取任何 `@localized` 字段的规则只在默认值轮执行一次；一个语言字段缺少
变体值也不会让相同规则用默认值再执行一次。

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
