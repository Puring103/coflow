# 表格 Source

Excel、CSV 和飞书/Lark 表格共享表格加载语义。表格 source 适合维护大量同构记录；复杂嵌套对象、数组、字典和覆盖模板通常更适合 [CFD](../cfd.md)。

## 基本规则

第一行是表头，后续行是数据。每一行对应一条顶层 record。

默认规则：

- sheet 名映射到 CFT type。
- `id`、`Id` 或 `ID` 列作为 record key。
- 表头文本映射到同名 CFT 字段。
- 单元格内容按目标字段类型使用 [单元格值语法](./cell-value.md) 解析。

表格 source 的第一行必须是表头。空数据行会被跳过。某个 sheet 的表头无法可靠映射时，该 sheet 的数据行会被跳过，但其他 sheet 和其他 source 仍会继续收集诊断。

## `sheets`

`sheets` 用来显式配置 sheet 到 type、key 列和字段名的映射：

```yaml
sources:
  - path: data/config.xlsx
    sheets:
      - sheet: Items
        type: Item
        key: Item ID
        columns:
          Display Name: name
          Price: price
```

| 字段 | 说明 |
| --- | --- |
| `sheet` | Excel worksheet、飞书 sheet 名，或表格 provider 的逻辑表名 |
| `type` | CFT type 名；省略时使用 sheet 名 |
| `key` | record key 表头列名；省略时使用 `id` / `Id` / `ID` |
| `columns` | 表头文本到 CFT 字段名的映射 |

未列入 `columns` 的表头仍会按原文本匹配字段。

显式配置 `key` 时，按配置值精确匹配表头；未配置时按 `id`、`Id`、`ID` 依次查找。key 列不映射到 CFT 字段。

`columns` 只做表头重命名，不限制未列出的字段。Coflow 会拒绝重复的源表头 key，避免 YAML map 后写覆盖导致配置被静默丢弃。

## `#` 控制列

表格可以包含名为 `#` 的控制列。数据行中该列单元格去掉首尾空白后等于 `##` 时，整行跳过。

这个控制列不映射到 CFT 字段，也不参与未知字段检查。

## `@expand`

CFT 字段标记 `@expand` 后，表格中可以把嵌套对象展开到相邻列。父字段列承载第一个子字段，后续相邻列必须连续且表头为空。

```text
sealed type Price {
  amount: int;
  currency: string;
}

type Item {
  @expand
  price: Price;
}
```

表格可以写成：

| id | price |  | name |
| --- | --- | --- | --- |
| sword | 100 | gold | Sword |

其中 `price` 列对应 `amount`，后续空表头列对应 `currency`。

`@expand` 字段必须引用具体对象类型，不能用于 nullable、数组、字典、enum 或 primitive 字段，也不能和 `@ref` 同时使用。

## Source 顺序

同一 type 的 records 按稳定顺序追加：

1. `coflow.yaml` 中 `sources` 的顺序。
2. 同一表格 source 内 `sheets` 的顺序。
3. 同一 sheet 内的行顺序。

这个顺序会影响 `data list`、导出文件中的记录顺序，以及编辑器展示顺序。

## 写回

表格 writer 会根据 record origin 定位原始行和列，再写回单元格文本。嵌套数组、字典、多态对象等复杂值会被渲染为可再次解析的单元格值文本。

写回仍会遵守 Provider 文件边界和 schema 约束，不会绕过数据源直接改 DataModel。
