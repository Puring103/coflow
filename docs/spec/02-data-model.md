# 数据模型

数据模型是 CFT 加载器的通用输出格式，与数据来源无关（Excel、JSON、数据库等均加载为同一模型）。

---

## 结构

```
DataModel {
  tables:            Map<TypeName, Table>,
  inheritance_index: Map<TypeName, PolymorphicIndex>,
  records:           Vec<Record>,
}

Table {
  type_name:     TypeName,
  records:       Vec<RecordId>,            // 保持数据源顺序；多个 sheet 映射同一类型时按配置文件顺序追加
  primary_index: Map<String, RecordId>,    // record key → RecordId
}

PolymorphicIndex {
  records:   Map<String, RecordId>,        // root type 赋值兼容范围内的所有可引用记录；root type 由所属 inheritance_index 的 key 确定
}

Record {
  key:         String,                      // 数据源记录 key；导出时写为保留字段 id
  actual_type: TypeName,                   // 运行时类型，用于 is 判断和继承 check
  fields:      Map<FieldName, Value>,
}

RecordId(usize)                            // 指向 DataModel.records 的稳定索引
```

公开 Rust API 中对应类型名为 `CfdDataModel`、`CfdTable`、
`CfdPolymorphicIndex`、`CfdRecord` 和 `CfdRecordId`。`CfdDataModel`
集中持有全部顶层记录；table 和 polymorphic index 只保存 `CfdRecordId`。
消费者通过 `model.record(id)`、`model.records()`、`model.records_of_type(type)`
或 `model.lookup(type, key)` 取回记录引用。

---

## Value 类型

```
Value =
  | Null
  | Bool(bool)
  | Int(i64)
  | Float(f64)
  | String(String)
  | Enum { enum_name: TypeName, variant: Option<String>, value: i64 }
  | Object(Box<Record>)                    // 内联嵌套对象，无独立 identity
  | Ref { key: String, target: RecordId }   // record-key 解析后的共享引用
  | Array(Vec<Value>)
  | Dict(Vec<(DictKey, Value)>)            // 保持插入顺序

DictKey =
  | String(String)
  | Int(i64)
  | Enum { enum_name: TypeName, variant: Option<String>, value: i64 }
```

**`Object` 和 `Ref` 的区别：**

- `Object`：内联嵌套对象，无独立 identity，不可被其他记录引用
- `Ref`：跨表引用，保留原始 record key 用于序列化和调试，同时持有目标 `RecordId`

内联对象只属于所在 Record，不可能被多处共享。

**标量 key 的限制：**

- 记录 key 固定为非空 `string`，由 Excel 特殊 `id` 列或其他 loader 的等价输入提供
- `DictKey` 只允许 `string`、`int`、`enum`，对应 CFT 字典 key 类型限制
- `Enum` 值必须携带 `enum_name`，因为不同枚举可以有相同 variant 名或相同底层整数值；比较、去重和字典 key 等价判断均以 `enum_name + value` 为准。`variant` 是展示提示，`@flag` 运算可能得到没有声明变体名的组合值，此时为 `None`
- `Float` 只允许有限 `f64` 值；`NaN`、`+/-inf` 不是合法数据值
- `Float` 不能作为字典 key

**字典 key 重复：**

字典在 schema-guided 解析后按 `DictKey` 判断重复。重复 key 是加载错误，不允许后写覆盖，也不保留多个同 key 条目。`Dict(Vec<...>)` 只用于保留合法字典的插入顺序。

---

## Record 的 actual_type

每个 Record 在加载时记录实际的 CFT 类型名。

- 非多态字段（字段声明类型是 `sealed type` 或无子类的普通 `type`）：`actual_type` 即字段声明的类型，无需数据源提供类型标记
- 多态字段（字段声明类型是 `abstract type` 或有子类的普通 `type`）：`actual_type` 由数据源中的类型标记提供（如单元格值语法中的 `TypeName{}`）；缺失则报错

`actual_type` 用于：
- `is` 类型判断
- 继承链上 check 块的依次执行（从根类到 actual_type 对应类）
- 代码生成时的类型分发（JSON 中的 `$type` 字段）

---

## 记录引用与继承树

对象字段如果输入为 `RecordRef { target_type, key }`，先在 `target_type` 的赋值兼容范围内按 record key 查找目标记录，再检查 `target_type` 能否赋给字段声明类型。Cell value 层的 `&key` 简写会被转换成 `target_type` 等于当前期望对象类型的 `RecordRef`：

- `TypeName` 是 `abstract type`：查找所有具体子类
- `TypeName` 是普通 `type` 且存在子类：查找该类型本身及所有子类
- `TypeName` 是 `sealed type` 或无子类的普通 `type`：只查找该类型本身

加载器为存在继承关系的类型建立 `inheritance_index`。每个 `PolymorphicIndex` 覆盖一个 root type 的赋值兼容范围，用于父类字段引用和跨子类 key 唯一性校验。

record key 必须是 string identifier。同一具体类型内 record key 必须唯一。同一 `PolymorphicIndex` 范围内的 record key 也必须唯一，否则父类字段引用无法判定目标。子类 key 可以满足父类字段；父类 key 不能满足子类字段。

路径引用 `PathRef { target_type, key, segments }` 先在 `target_type` 的赋值兼容范围内按 record key 找到根记录，再按字段访问和数组/字典索引访问定位值。路径结果仍必须与目标字段类型兼容。`&key` 简写不会生成 `PathRef`；路径引用必须带显式根类型。

---

## 多条数据来源的顺序

同一类型的 records 按以下顺序追加：

1. 按配置文件 `sources` 列表的顺序
2. 同一 file 内按 `sheets` 列表的顺序
3. 同一 sheet 内按行序

---

## Singleton 校验

被 `@singleton` 标记的 type 在数据模型构建阶段（`CfdDataModel` build）执行下列校验，与具体数据来源无关：

- 该 type 的 records 数量必须 = 1，否则报 `CFD-DATA-015 SingletonRecordCountInvalid`（多于 1 条与少于 1 条共用此错误码，附带数量信息）
- record key 必须显式提供且为合法 CFT 标识符；否则报 `CFD-DATA-016 SingletonKeyMissingOrInvalid`
- 项目中所有 `@singleton` type 的 record key 互不相同（跨 type 全局唯一）；否则报 `CFD-DATA-017 SingletonKeyCollision`，相关位置指向首次出现

---

## Localized record key 校验

包含 `@localized` 字段的 record，其 record key 仍需满足通用的 record key 规则（合法 CFT 标识符），由现有 `CFD-DATA-013 InvalidRecordKey` 路径覆盖。`CFD-DATA-014` 当前保留，等到 record key 与翻译表 key 规则发生分化时再启用。

---

## CFD-DATA 错误码增量

| 错误码 | 名称 | 含义 |
|--------|------|------|
| `CFD-DATA-014` | `LocalizedRecordKeyInvalid` | 保留：将来用于 record key 与翻译 key 规则分化 |
| `CFD-DATA-015` | `SingletonRecordCountInvalid` | `@singleton` type 的 records 数量不等于 1 |
| `CFD-DATA-016` | `SingletonKeyMissingOrInvalid` | `@singleton` type 的 record key 缺失或非合法 CFT 标识符 |
| `CFD-DATA-017` | `SingletonKeyCollision` | 不同 `@singleton` type 的 record key 撞名 |
