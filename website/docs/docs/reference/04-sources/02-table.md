# 表格 Source

Excel 和 CSV 表格共享表格加载语义。表格 source 适合维护大量同构记录；复杂嵌套对象、数组、字典和覆盖模板通常更适合 [CFD](../03-language/02-cfd.md)。

## 基本规则

第一行是表头，后续行是数据。每一行对应一条顶层 record。

默认规则：

- sheet 名映射到 CFT type。
- `id`、`Id` 或 `ID` 列作为 record key。
- 表头文本映射到同名 CFT 字段。
- 单元格内容按目标字段类型使用 [单元格值语法](../03-language/03-cell-value.md) 解析。

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
| `sheet` | Excel worksheet 或表格 provider 的逻辑表名 |
| `type` | CFT type 名；省略时使用 sheet 名 |
| `key` | record key 表头列名；省略时使用 `id` / `Id` / `ID` |
| `columns` | 表头文本到 CFT 字段名的映射 |

未列入 `columns` 的表头仍会按原文本匹配字段。

显式配置 `key` 时，按配置值精确匹配表头；未配置时按 `id`、`Id`、`ID` 依次查找。key 列不映射到 CFT 字段。

`columns` 只做表头重命名，不限制未列出的字段。Coflow 会拒绝重复的源表头 key，避免 YAML map 后写覆盖导致配置被静默丢弃。

同一个 source 中，`sheet` 名必须唯一，同一个 CFT 字段也不能被多个
`columns` 项同时映射。一个 type 可以显式映射到多张 sheet，但执行创建、同步
或插入等写操作时必须通过 `--sheet` 指定目标；Coflow 不会按配置顺序静默选择
第一张 sheet。显式给出的 sheet 若配置成另一个 type，也会在写入前报错。

## 配置案例

### 默认映射

当 sheet 名与 CFT type 相同、表头与字段名相同时，只需配置文件路径：

```yaml
sources:
  - path: data/items.xlsx
```

`items.xlsx` 中的 `Item` sheet：

| id | name | rarity | price |
| --- | --- | --- | --- |
| potion | Potion | Common | 50 |
| sword | Iron Sword | Rare | 120 |

Coflow 将 sheet 映射为 `Item` type，将 `id` 作为 record key，其余列映射到同名字段。

### 展示名表头

表格使用面向策划的名称时，通过 `type`、`key` 和 `columns` 映射：

```yaml
sources:
  - path: data/items.xlsx
    sheets:
      - sheet: 物品表
        type: Item
        key: 物品ID
        columns:
          名称: name
          稀有度: rarity
          价格: price
```

对应表格：

| 物品ID | 名称 | 稀有度 | 价格 |
| --- | --- | --- | --- |
| potion | Potion | Common | 50 |
| sword | Iron Sword | Rare | 120 |

未列入 `columns` 的表头仍会按原名称匹配字段，因此只需配置实际发生重命名的列。

### 一个 workbook 包含多个 type

```yaml
sources:
  - path: data/gameplay.xlsx
    sheets:
      - sheet: 物品表
        type: Item
        key: 物品ID
        columns:
          名称: name
          稀有度: rarity
      - sheet: 怪物表
        type: Monster
        key: 怪物ID
        columns:
          等级: level
          掉落: drop
```

每个 sheet 独立映射 type 和 key 列。读取顺序与 `sheets` 中的声明顺序一致。

### 目录中混合多种数据源

```yaml
sources:
  - path: data
    sheets:
      - sheet: 物品表
        type: Item
        key: 物品ID
        columns:
          名称: name
```

如果 `data/` 中包含 `items.xlsx`、`monsters.csv` 和 `story.cfd`，Coflow 会递归发现这些文件。`sheets` 映射作用于表格文件；CFD records 仍由文本中的类型声明决定。

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

`@expand` 字段必须是具体内联对象类型，不能用于 `&Type` 引用、nullable、数组、字典、enum 或 primitive 字段。

## Source 顺序

同一 type 的 records 按稳定顺序追加：

1. `coflow.yaml` 中 `sources` 的顺序。
2. 同一表格 source 内 `sheets` 的顺序。
3. 同一 sheet 内的行顺序。

这个顺序会影响 `data list`、导出文件中的记录顺序，以及编辑器展示顺序。

## 写回

表格 writer 会根据 record origin 定位原始行和列，再写回单元格文本。嵌套数组、字典、多态对象等复杂值会被渲染为可再次解析的单元格值文本。

写回仍会遵守 Provider 文件边界和 schema 约束，不会绕过数据源直接改 DataModel。

## 创建表格

`coflow data create-table` 可以在已有表格 source 中创建新的 sheet/table，并按 CFT type 写入表头。

```powershell
coflow data create-table <project> --source data/gameplay.xlsx --type Item --sheet Item
```

它和 `data create-file` 的边界不同：

- `data create-file` 创建新的本地 `.cfd`、`.csv` 或 Excel 文件。
- `data create-table` 在已有 Excel workbook 中创建一个 sheet。

创建表格只需要 schema，不加载完整 DataModel，也不执行 `check {}`。创建后如果要确认数据源整体可用，继续运行 `coflow check`。
