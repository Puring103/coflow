# 数据模型

数据模型是 CFT 加载器的通用输出格式，与数据来源无关（Excel、JSON、数据库等均加载为同一模型）。

---

## 结构

```
DataModel {
  tables:            Map<TypeName, Table>,
  inheritance_index: Map<TypeName, PolymorphicIndex>,
}

Table {
  type_name:     TypeName,
  records:       Vec<Rc<Record>>,          // 保持数据源顺序；多个 sheet 映射同一类型时按配置文件顺序追加
  primary_index: Map<IdValue, Rc<Record>>, // @id 字段值 → Record；每个继承树最多一个 @id 字段
}

PolymorphicIndex {
  root_type: TypeName,
  records:   Map<IdValue, Rc<Record>>,     // root_type 赋值兼容范围内的所有可引用记录
}

Record {
  actual_type: TypeName,                   // 运行时类型，用于 is 判断和继承 check
  fields:      Map<FieldName, Value>,
}
```

---

## Value 类型

```
Value =
  | Null
  | Bool(bool)
  | Int(i64)
  | Float(f64)
  | String(String)
  | Enum { enum_name: TypeName, variant: String, value: i64 }
  | Object(Box<Record>)                    // 内联嵌套对象，无独立 identity
  | Ref { id: IdValue, target: Rc<Record> } // @ref 解析后的共享引用
  | Array(Vec<Value>)
  | Dict(Vec<(DictKey, Value)>)            // 保持插入顺序

IdValue =
  | String(String)
  | Int(i64)

DictKey =
  | String(String)
  | Int(i64)
  | Enum { enum_name: TypeName, variant: String, value: i64 }
```

**`Object` 和 `Ref` 的区别：**

- `Object`：内联嵌套对象，无独立 identity，不可被其他记录引用。字段类型是 `type` 且无 `@ref` 注解时，值为 `Object`
- `Ref`：跨表引用，保留原始 ID 用于序列化和调试，同时持有目标 Record 的共享引用。字段有 `@ref` 注解时，值为 `Ref`

内联对象只属于所在 Record，不可能被多处共享。

**标量 key 的限制：**

- `IdValue` 只允许 `string` 和 `int`，对应 `@id` / `@ref` 的字段类型限制
- `DictKey` 只允许 `string`、`int`、`enum`，对应 CFT 字典 key 类型限制
- `Enum` 值必须携带 `enum_name`，因为不同枚举可以有相同 variant 名或相同底层整数值；比较、去重和字典 key 等价判断均以 `enum_name + value` 为准
- `Float` 不能作为 `@id`、`@ref` 或字典 key

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

## @ref 与继承树

`@ref(TypeName)` 的查找范围是 `TypeName` 的赋值兼容范围：

- `TypeName` 是 `abstract type`：查找所有具体子类
- `TypeName` 是普通 `type` 且存在子类：查找该类型本身及所有子类
- `TypeName` 是 `sealed type` 或无子类的普通 `type`：只查找该类型本身

加载器为存在继承关系的类型建立 `inheritance_index`。每个 `PolymorphicIndex` 覆盖一个 root type 的赋值兼容范围，用于 `@ref(root)` 和跨子类 ID 唯一性校验。

如果某个类型继承树中存在 `@id` 字段，则该 `@id` 字段由声明它的祖先类型定义，并被所有子类继承。同一 `PolymorphicIndex` 范围内的 `IdValue` 必须唯一。子类不能重新声明另一个 `@id` 字段。

---

## 多条数据来源的顺序

同一类型的 records 按以下顺序追加：

1. 按配置文件 `sources` 列表的顺序
2. 同一 file 内按 `sheets` 列表的顺序
3. 同一 sheet 内按行序
