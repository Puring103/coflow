# Unity 原生集成

> 新 VM 的设计与验收以 [新 VM 实施计划](../plans/new-vm-and-csharp-snapshot.zh-CN.md) 及其配套规格为准；本文冲突内容在同一次交付中同步。

## 包与 ABI

Unity 包目录为 `engine/runtimes/csharp/src/Coflow.Runtime`，最低 Unity 版本为 2022.1，
API 级别为 .NET Standard 2.1。包装使用固定签名的 C ABI、创建线程拥有型句柄、静态回调和
`AOT.MonoPInvokeCallback`；生成类型直接调用静态工厂，不使用 Reflection.Emit 或运行时泛型代码生成。

ABI 定义位于 `engine/crates/coflow-ffi/include/coflow.h`。所有文本为 UTF-8，长度不含结尾零。
句柄使用 64 位整数，指针和长度使用目标平台的指针宽度；返回结构使用 C 自然对齐。
返回缓冲区由调用方复制后释放，Host 回调返回的缓冲区所有权交给 Rust。

## 原生库

从仓库根目录构建 `coflow-ffi`，按 Unity Player 的目标平台选择 Rust target：

```sh
cargo build -p coflow-ffi --release --target <target>
```

该包默认关闭 CFT 声明编译器；开发工具需要原生 CFT 编译入口时启用 `cft-compiler` feature。
Windows 加载 `coflow_ffi.dll`，Linux/Android 加载 `libcoflow_ffi.so`，macOS 加载
`libcoflow_ffi.dylib`。iOS Player 通过 `__Internal` 静态链接 `libcoflow_ffi.a`；
编辑器仍使用桌面动态库。原生库必须与 Player 的操作系统、CPU 和 ABI 一致。

Host 回调委托由静态字段保活，服务对象由 GCHandle 保活。服务释放回调只释放 GCHandle，
不访问 Unity API。异常在托管回调内部捕获并返回错误，不能跨 C ABI 抛出。

## 交付验收

IL2CPP 验收使用实际 Player 构建，覆盖契约读取、CFD 构建、生成对象访问、集合访问、
Host 读取与缺失绑定、显式释放及垃圾回收保活。同线程同步重入与并发忙错误分别验收。
每个交付平台保留 Unity 版本、Player 设置、Rust target 和执行结果。
桌面 .NET 构建不能替代 IL2CPP Player 验收。
