# 示例

仓库中的示例分别覆盖完整项目、语法、本地化和纯 CFD 数据。第一次使用建议从 `examples/rpg` 开始。

## 运行 RPG 示例

```powershell
coflow cft check examples/rpg
coflow check examples/rpg
coflow build examples/rpg
```

- `cft check` 只检查 CFT schema。
- `check` 加载全部 source，构建 data model 并执行业务规则。
- `build` 在同样的检查之后发布 JSON 和 C# 产物。

该示例的输出目录是 `examples/rpg/generated/data` 和 `examples/rpg/generated/csharp`。这些目录由 Coflow 整体接管，不要放入手写文件。

## 其他示例

| 目录 | 用途 |
| --- | --- |
| `examples/cft` | CFT 常用语法和 check 表达式 |
| `examples/cfd` | CFD 记录、多态对象、路径和 spread |
| `examples/localization` | language dimension、变体文件和 C# runtime |
| `examples/card_game` | 小型纯文本项目 |
| `examples/workflow` | Excel、CFD、维度和稳定 enum lock 的综合流程 |

更完整的数据结构说明见 [RPG 示例](../../examples/rpg.md)。
