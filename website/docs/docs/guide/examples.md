# CFD 示例

仓库只保留 CFD 输入示例。它们不生成数据文件，代码生成目录只包含目标语言源文件。

```powershell
coflow cft check examples/cfd
coflow check examples/cfd
coflow build examples/cfd
```

- `cft check` 只检查 CFT schema。
- `check` 加载 `.cfd`，构建数据模型并执行业务规则。
- `build` 在检查成功后发布 C# 等目标语言源文件。

输出目录是 `examples/cfd/generated/csharp`。该目录由 Coflow 整体接管，不要放入手写文件。

## 其他示例

| 目录 | 用途 |
| --- | --- |
| `examples/cft` | CFT 语法和 check 表达式 |
| `examples/cfd` | CFD 记录、多态对象、路径和维度 |
| `examples/card_game` | 小型纯文本项目 |
