# C# Loader 集成验证

该工程验证生成代码通过 Coflow.Loader 加载调用方选择的 CFD 文件，直接得到业务类型。
验证结构体、继承记录、引用成环、共享对象身份、默认值、枚举组合、集合、基础插值及 For 维度读取。

```sh
cargo run -- codegen tests/csharp-loader-integration
dotnet run --project tests/csharp-loader-integration/app/Coflow.Loader.Example.csproj --configuration Release
```
