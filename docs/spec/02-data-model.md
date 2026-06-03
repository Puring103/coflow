# 数据模型

数据模型是 CFT 加载器的通用输出格式，与数据来源无关（Excel、JSON、数据库等均加载为同一模型）。

---

## 结构

```
DataModel {
  tables: Map<Name, Table>,
}

Table {
  type_name:     TypeName,
  records:       Vec<Rc<Record>>,          // 保持数据源顺序；多个 sheet 映射同一类型时按配置文件顺序追加
  primary_index: Map<Value, Rc<Record>>,   // @id 字段值 → Record；每个类型只能有一个 @id 字段
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
  | Enum { variant: String, value: i64 }
  | Object(Record)                         // 内联嵌套对象，无独立 identity
  | Ref { id: Value, target: Rc<Record> }  // @ref 解析后的共享引用
  | Array(Vec<Value>)
  | Dict(Vec<(Value, Value)>)              // 保持插入顺序
```

**`Object` 和 `Ref` 的区别：**

- `Object`：内联嵌套对象，无独立 identity，不可被其他记录引用。字段类型是 `type` 且无 `@ref` 注解时，值为 `Object`
- `Ref`：跨表引用，保留原始 ID 用于序列化和调试，同时持有目标 Record 的共享引用。字段有 `@ref` 注解时，值为 `Ref`

内联对象只属于所在 Record，不可能被多处共享。

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

`@ref(TypeName)` 中 TypeName 可以是 `abstract type`，表示持有该继承树中任意子类实例的 ID。加载器在该继承树所有子类的 `primary_index` 中查找目标。所有子类的 `@id` 字段值在整个继承树中必须唯一。

---

## 多条数据来源的顺序

同一类型的 records 按以下顺序追加：

1. 按配置文件 `sources` 列表的顺序
2. 同一 file 内按 `sheets` 列表的顺序
3. 同一 sheet 内按行序
