# C# 原生运行时集成夹具

此夹具验证生成目录中的 Rust 二进制契约、CFD 数据、生成的 C# 包装、记录引用、data struct、集合、函数调用、
模板求值、Host 函数、check 及释放行为。

从仓库根目录运行：

```sh
tests/csharp-runtime-integration/test.sh
```

脚本构建原生库、生成类型并运行桌面包装层测试。Unity 2022+ / IL2CPP 另按
`docs/2-架构/07-Unity原生集成.md` 在实际 Player 中验收。
