# C# Loader 集成验证

该工程验证生成代码通过 Coflow.Loader 加载调用方选择的 CFD 文件，直接得到业务类型。
验证 table/data/singleton 的边界、partial 扩展、Table<T>() 与 Singleton<T>()、枚举键 Get/TryGet、继承与多态、引用成环、默认值、集合、基础插值及 For 维度读取。data 结构体不创建记录身份类。

该工程与 Rust loader 共同消费 `tests/fixtures/cfd-runtime-parity`，核对全部合法和非法输入的加载结果，并验证 int64/binary64 精度、嵌套 Option 默认值及覆盖层次。parser parity 夹具由 Rust language 测试与 C# Loader.Tests 共同消费。

```sh
cargo run -- codegen tests/csharp-loader-integration
dotnet run --project tests/csharp-loader-integration/app/Coflow.Loader.Example.csproj --configuration Release
```
