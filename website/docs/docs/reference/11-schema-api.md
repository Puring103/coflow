# Schema API

Schema API 是 CFT compiler 的只读语义模型，供 runtime、checker、codegen、LSP 和 editor 使用。它不选择输入格式、不读文件、不写产物。

核心视图包括：

- `CftSchema`：类型、字段、enum、const、继承和 dimension metadata。
- `CftModuleSet`：模块 id、规范化路径、原文和 AST span。
- `SchemaFieldInfo`、`SchemaTypeInfo`：供 IDE 和 generator 查询的稳定索引。
- `ValueDependencyPlan`：默认值和 check 的依赖顺序及循环诊断。

目标语言 generator 通过 `CodegenInput { schema, model, sources, target }` 消费这些只读结构；CFT 层不依赖任何 runtime 或目标语言 crate。
