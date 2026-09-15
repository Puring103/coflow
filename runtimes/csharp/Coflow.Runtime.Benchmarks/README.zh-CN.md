# C# 原生数据访问基准

基准测量生成类型经 C ABI 读取 Rust 数据的成本。先构建 `coflow-ffi` 的 release 库，
再运行 `cargo run -- codegen tests/csharp-runtime-integration` 生成夹具类型。
使用此目录中的项目启动 BenchmarkDotNet。基准不执行函数或虚拟机，不替代 Unity Player 验收。
