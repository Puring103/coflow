# 安装与快速开始

```powershell
cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --help
```

```powershell
coflow check examples/showcase
coflow codegen examples/showcase
```

项目至少包含一个 `.cft` schema、一个 `.cfd` 数据路径和一个 `codegen` target。生成 C# 后，将 `runtimes/csharp/Coflow.Cfd.Runtime` 引入目标项目，并调用生成数据库的 `Load` overload。
