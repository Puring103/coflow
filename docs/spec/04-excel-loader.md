# Excel 加载器规格

**依赖文档**：[01-cft.md](01-cft.md)、[02-data-model.md](02-data-model.md)、[03-cell-value.md](03-cell-value.md)

Excel 加载器将 `.xlsx` 文件按 CFT schema 加载为结构化数据模型。加载过程完全 schema-guided，所有类型解析、单元格值解析和跨表引用解析均依赖 CFT 类型定义。

---

## 目录

1. [配置文件](#1-配置文件)
2. [加载流程](#2-加载流程)
3. [单元格值解析](#3-单元格值解析)
4. [错误阶段](#4-错误阶段)

---

## 1. 配置文件

配置文件使用 YAML 格式，描述 schema 来源和数据来源。

### 1.1 基本结构

```yaml
schema: schema/           # .cft 文件所在文件夹，扫描全部 .cft 文件

sources:
  - file: data/items.xlsx
    sheets:
      - sheet: Item       # sheet 名和 CFT 类型名一致，无需额外配置

  - file: data/enemies.xlsx
    sheets:
      - sheet: Monster
```

### 1.2 `schema` 字段

`schema` 指定 CFT 类型定义的来源，支持两种写法：

```yaml
# 扫描整个文件夹下所有 .cft 文件
schema: schema/

# 或显式列出文件
schema:
  - schema/item.cft
  - schema/enemy.cft
```

路径相对于配置文件所在目录。

### 1.3 `sources` 字段

每个 source 对应一个 `.xlsx` 文件，包含若干 sheet 配置。

**sheet 配置字段：**

| 字段 | 必填 | 说明 |
|------|------|------|
| `sheet` | 是 | Excel 中的 sheet 名 |
| `type` | 否 | 对应的 CFT 类型名；省略时使用 sheet 名作为类型名 |
| `columns` | 否 | 列名到字段名的映射；省略时列名直接作为字段名 |

`columns` 映射只处理列名到字段名的转换，不干涉单元格内容的解析。多态字段的类型标记（`TypeName{}`）由单元格内容本身提供。

### 1.4 完整示例

```yaml
schema: schema/

sources:
  - file: data/items.xlsx
    sheets:
      - sheet: Item                 # sheet 名 = 类型名，无需额外配置

      - sheet: 物品表               # sheet 名与类型名不一致
        type: Item
        columns:                    # 列名与字段名不一致时才需要填
          物品ID: id
          名称: name
          稀有度: rarity

  - file: data/enemies.xlsx
    sheets:
      - sheet: Monster
        columns:
          怪物ID: id
          等级: level
```

---

## 2. 加载流程

加载结果为 `DataModel`，结构定义见 [02-data-model.md](02-data-model.md)。

加载分为以下几个阶段，任一阶段出错则停止并报告错误。

### 第一阶段：加载 schema

扫描并注册所有 `.cft` 文件到 `CftContainer`，建立全局类型表。

### 第二阶段：解析 Excel 结构

读取所有配置中的 `.xlsx` 文件，按 sheet 配置确定每个 sheet 对应的 CFT 类型。

每个 sheet 的**第一行**为列名行，后续行为数据行。空行跳过。

### 第三阶段：逐行解析记录

对每个数据行，按列名（经过 `columns` 映射后）找到对应字段，根据字段的 CFT 类型 schema-guided 解析单元格内容（见第 3 节）。

解析结果构造为 `Record`，加入对应 `Table` 的 `records` 列表。同时将 `@id` 字段值注册到该类型的 `primary_index`，重复 ID 立即报错。

如果该类型属于带 `@id` 的继承树，加载器还要把记录注册到相关 `inheritance_index`。同一继承树索引中的 ID 必须唯一，兄弟子类之间重复 ID 也立即报错。

同一类型的 records 按配置文件中 sources 顺序追加（见 [02-data-model.md](02-data-model.md)）。

### 第四阶段：解析跨表引用

遍历所有 Record，将 `@ref` 字段的值（string 或 int）按目标类型的赋值兼容范围查找，替换为 `Value::Ref { id, target }`。

- `@ref(TypeName)` 中 TypeName 是 `sealed type` 或无子类的普通 `type` 时，在该类型的 `primary_index` 中查找
- `@ref(TypeName)` 中 TypeName 是 `abstract type` 或有子类的普通 `type` 时，在对应 `inheritance_index` 中查找
- 允许循环引用（A.ref → B，B.ref → A）；两遍设计天然支持，不会无限递归
- 找不到目标则报错

### 第五阶段：执行 check

对所有 Record 执行 CFT check 块校验，收集全部错误后一次性返回。check 错误不影响数据模型的构建结果，由宿主决定是否使用含错误的数据。

---

## 3. 单元格值解析

单元格值语法详见 [03-cell-value.md](03-cell-value.md)。

解析器是完全 schema-guided 的，每个单元格的目标类型由对应列的 CFT 字段类型确定。

**特殊情况：**

| 单元格内容 | 语义 |
|-----------|------|
| 空单元格 | 字段有默认值时使用默认值；无默认值且非 nullable 时报错 |
| `_` | 同空单元格 |
| `null` | 显式填 null；字段类型必须是 `T?`，否则报错 |

---

## 4. 错误阶段

| 阶段 | 错误类型 |
|------|---------|
| schema 加载 | CFT 解析错误、类型名重复 |
| Excel 解析 | 文件不存在、sheet 不存在、`type` 指定的类型未定义 |
| 记录解析 | 列名找不到对应字段、单元格值类型不匹配、`@id` 重复、继承树 ID 重复、字典 key 重复、多态字段缺少类型标记 |
| 跨表引用解析 | `@ref` 目标类型不存在、目标 ID 找不到 |
| check 执行 | 条件为假、类型错误、越界 |
