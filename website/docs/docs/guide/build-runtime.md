# 代码生成与运行时

`coflow check` 验证 schema、CFD、引用和 `check {}`。`coflow codegen` 加载数据模型并原子发布目标语言源文件，但不执行 `check {}`。交付前先运行 `coflow check`。

C# 运行时包 `Coflow.Runtime` 提供 Module 管理、CFD 解析、编译、查询和函数执行。生成 binding 负责 Schema 对应的强类型转换；运行时不需要 Rust 或 CFT。
