# 构建与运行时

`coflow check` 只验证 schema、CFD、引用和 check。`coflow codegen` 生成目标语言源文件，`coflow build` 组合两者并原子替换代码目录。

C# 运行时包 `Coflow.Cfd.Runtime` 提供 tokenizer、parser、span、文本读取、引用缓存和资源限制。生成 binding 负责 schema-specific 类型转换；运行时不需要 Rust 或 CFT。
