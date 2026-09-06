# 构建与运行时

`coflow check` 只验证 schema、CFD、引用和 check。`coflow codegen` 生成目标语言源文件，`coflow build` 组合两者并原子替换代码目录。

C# 运行时包 `Coflow.Runtime` 提供 Module 管理、CFD 解析、编译、查询和函数执行。生成 binding 负责 Schema 对应的强类型转换；运行时不需要 Rust 或 CFT。
