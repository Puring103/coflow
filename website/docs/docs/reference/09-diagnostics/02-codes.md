# 错误码索引

| 阶段 | 示例 |
| --- | --- |
| `PROJECT` | `PROJECT-CONFIG-PARSE`、`PROJECT-MISSING-DATA`、`PROJECT-CODEGEN-TARGET` |
| `CFT` | `CFT-PARSE`、`CFT-TYPE`、`CFT-CHECK` |
| `CFD` | `CFD-PARSE`、`CFD-UNKNOWN-FIELD`、`CFD-DUPLICATE-RECORD` |
| `MODEL` | `MODEL-REFERENCE`、`MODEL-TYPE`、`MODEL-DUPLICATE-KEY` |
| `CODEGEN` | `CODEGEN-OPTIONS`、`CODEGEN-ARTIFACT-PATH` |
| `RUNTIME` | `RUNTIME-LIMIT`、`RUNTIME-SOURCE-MISSING` |

错误码是稳定机器接口；具体字段和位置从诊断结构读取，不从文本消息匹配。

数据模型内部的细分错误码也保持稳定，例如记录引用环为 `REF-003`。生成的 C# runtime 使用其公开 runtime 诊断命名空间报告同一约束，例如 `CFD-REF-CYCLE`；两者的字符串名称不要求相同，接受/拒绝语义必须一致。
