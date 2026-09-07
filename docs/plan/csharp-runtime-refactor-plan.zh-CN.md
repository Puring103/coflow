# C# Runtime 精简与性能优化

## 架构边界

Runtime 使用单一生产链：

```text
生成元数据 + CFD
  -> 函数分析
  -> typed CFG
  -> 寄存器分配与链接
  -> 已验证程序
  -> 寄存器 VM
```

- Schema 拥有类型、字段、codec 和布局。
- SnapshotBuilder 完成加载与发布，Snapshot 保存发布后的数据和程序。
- 函数分析器拥有语法游标、作用域和绑定状态；公共类型规则集中复用。
- CFG lowering 与寄存器分配各自处理编译产物，不读取 parser 的可变状态。
- FunctionTarget 统一分派程序、Host 和 closure；生成入口只传对象 ID 与函数参数。
- RegisterStorage 拥有寄存器存储，FrameStack 拥有调用帧。
- Arena 拥有集合数据及索引，冻结副本共享构建后不再修改的字典索引。

正文分析保持一条完整实现，不要求额外构造 syntax、bound、typed 三套正文树。
组件只有在减少重复逻辑或建立真实状态所有权时才拆分；不以文件数量或文件长度验收。

## 信任边界

用户输入的语法、类型、实际索引及句柄生命周期错误在对应边界报告。
编译器构造的字段存储类别在最终程序构建时检查一次，VM 执行时直接使用。
内部调用不增加兼容分支、自动导入模式或新的外部 receiver API。

## 本轮优化范围

1. 合并嵌套正文 binder，删除额外对象、回调与文件。
2. 删除生成调用中未使用的 receiver 泛型和参数，同步生成模板、API 基线及全部调用方。
3. 将函数目标分派集中到 FunctionTarget，删除 FunctionEntry 的重复程序判定。
4. 合并等价寄存器偏移 helper。
5. 函数定义去重使用 HashSet，保持首次出现顺序。
6. 字典针对现有整数、枚举和字符串 key 构建索引，保持重复 key 首次匹配行为。
7. 字典编码只枚举一次，删除保存全部 pair 的中间数组。
8. 将字段 Host/Arena 静态类别检查集中到 executable verifier。

本轮不改变语言的相等规则、Host 值生命周期或平台支持范围。

## 验收

- Runtime 的 net8.0、netstandard2.1 构建及 Runtime 测试。
- netstandard smoke、生成器测试及两个 C# 集成应用。
- 两个样例连续生成结果一致。
- 索引覆盖整数、字符串、缺失 key、重复 key、冻结和源存储清理。
- Benchmark 项目构建；性能结论区分算法复杂度和实测吞吐。
- 仓库根目录执行 cargo check --workspace、cargo test --workspace。
- git diff --check。
