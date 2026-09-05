# Showcase 示例

仓库只保留一个用户示例项目。每个编号 CFT 文件介绍一种 schema 特性，
同编号 CFD 文件展示对应的数据写法。

```powershell
coflow cft check examples/showcase
coflow check examples/showcase
coflow build examples/showcase
```

- `cft check` 只检查 CFT schema。
- `check` 加载 `.cfd`，构建数据模型并执行业务规则。
- `build` 在检查成功后发布 C# 等目标语言源文件。

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
| `13-functions` | 函数值和函数体 |
