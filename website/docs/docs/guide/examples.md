# Showcase 示例

仓库只保留一个用户示例项目。每个编号 CFT 文件介绍一种 schema 特性，
同编号 CFD 文件展示对应的数据写法。

```powershell
coflow schema check --project examples/showcase
coflow check --rules --project examples/showcase
coflow codegen --project examples/showcase
```

- `schema check` 只检查 CFT schema。
- `check --rules` 加载 `.cfd`，构建数据模型并执行业务规则。
- `codegen` 生成 C# 源文件；`build` 在全部业务规则通过后生成代码。

输出目录是 `examples/showcase/generated/csharp`。该目录由 Coflow 整体接管，不要放入手写文件。

## 文件划分

| 编号 | 特性 |
| --- | --- |
| `01-records` | 记录和标量字段 |
| `02-defaults` | 字段默认值 |
| `03-enums` | 普通枚举 |
| `04-flags` | flag 枚举 |
| `05-arrays` | 数组 |
| `06-dictionaries` | 字典 |
| `07-inheritance` | 继承、抽象类型和多态对象 |
| `08-references` | 记录引用 |
| `09-options` | 可选值 |
| `10-checks` | 校验表达式 |
| `11-conditional-checks` | 条件校验 |
| `12-quantifiers` | 集合量词 |
| `14-dimensions` | 本地化字段和语言覆盖 |
