# Data model

runtime 将 CFD 解析为 schema-guided、不可变的 `CfdDataModel`。记录 identity 是 `(source_id, record_index)`，引用 identity 是 `(declared_type, key)`；模型不携带目标语言或文件格式特例。
