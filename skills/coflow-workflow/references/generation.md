# 代码生成与交付

先通过 project inspect 核对 codegen 目标和输出目录。当前内置目标为 C#，应用接入见 [C# 接入](csharp-integration.md)。

```yaml
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

生成目录应专用，与所有输入及其他生成目录分离。业务 partial 文件放在目录外；不要手工修改生成文件。

## 按目标选择命令

| 目标 | 命令 | 检查与写入 |
| --- | --- | --- |
| 生成代码 | `coflow codegen --project <项目> --json` | 结构与数据有效后生成，不执行全部业务规则 |
| 预览生成 | `coflow codegen --project <项目> --dry-run --json` | 返回候选变化，不写入 |
| 检查生成物一致性 | `coflow codegen --project <项目> --check --json` | 不更新产物；不一致会失败 |
| 完整校验并生成 | `coflow build --project <项目> --json` | 全部业务规则通过后生成 |

任务要求交付可用配置时优先 build；仅检查项目用 check --rules。生成不是 CFD 数据导出，应用加载原始 CFD 文本与生成的类型。

build/codegen 可携带 if-revision 保护此前读取的输入。产物发布失败时查看实际写入与恢复结果，处理方式同[校验与恢复](validation-and-recovery.md)。

## 记录枚举与应用变化

`@idAsEnum` 的映射随当前记录更新，已有编号保持稳定。记录改名使用 data rename，避免手工文本替换遗漏映射或引用。应用中的枚举成员、字符串 ID 及业务 partial 调用仍需要按业务范围同步。

CFT 结构改变后，重新生成并编译消费这些类型的应用；仅 CFD 值改变可由应用重新加载。C# Loader 不执行业务 check，数据交付前的 check --rules/build 仍有必要。

## 配置项目 CI

需要生成输出的项目运行 build。生成文件已纳入版本控制且 CI 只核对时，运行 check --rules，再运行 codegen --check。输出成功已包含对应检查，避免紧接着重复同范围校验。

任务要求格式化时可用 `coflow format --project <项目> --check --json` 或 `--dry-run` 查看差异，再正式格式化；格式化不代替语义校验。
