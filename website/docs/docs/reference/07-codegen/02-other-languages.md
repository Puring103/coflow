# C++、Lua、GDScript 与 Rust

新增代码生成 provider：

| `code.type` | 目标 | JSON loader | Protobuf loader |
| --- | --- | --- | --- |
| `cpp` | 保守 C++11 | `cpp-json` | `cpp-protobuf` |
| `lua` | Lua 5.1 | `lua-json` | 暂不支持 |
| `gdscript` | Godot 4 | `gdscript-json` | 暂不支持 |
| `rust` | Rust 2021 | `rust-json` | `rust-protobuf` |

C++ Protobuf loader 是无异常、无 Google Protobuf runtime 依赖的 schema-specific reader。Rust
Protobuf loader 同样内置小型 wire reader。Loader 仍由 `(code.type, data.type)` 自动选择，不需要
单独配置 runtime 版本。

JSON 中枚举使用符号字符串；四种 loader 同时接受旧整数枚举作为迁移兼容。Lua 和 GDScript
会拒绝超出 IEEE-754 精确整数范围的 JSON 整数。

