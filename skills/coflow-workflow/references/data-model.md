# CFD 数据模型

CFD 解析先得到语法节点，再按 CFT schema lower 为稳定的数据模型。模型不包含表格行号或目标语言格式。

核心 identity 是 `(source_id, record_index)`；记录引用使用 `(declared_type, key)`，诊断位置来自 source span。

```text
CfdDataModel
  records: RecordIndex
  values: CfdValue
  source_index: SourceIndex
  diagnostics: DiagnosticSet
```

`CfdValue` 覆盖 null、bool、整数、浮点、字符串、enum、引用、数组、字典和对象。默认值、继承、多态、维度 overlay 和 check 在 schema-guided lower/check 阶段完成；目标语言 generator 只读取最终 schema/model。

重复记录、未知字段、缺失必填字段、错误引用和 check 失败均是带 source span 的诊断。模型发布是不可变操作，失败尝试不能覆盖上一份成功 generation。
