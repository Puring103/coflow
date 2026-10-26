# 调整项目输入与生成目标

先用 project inspect 查看配置，再用 project files 核对实际输入。项目根为 coflow.yaml 所在目录；配置路径相对此处，命令的 input 文件相对调用目录。

```yaml
schema:
  - schema/
  - ../shared/common.cft
data: data/
dimensions:
  language:
    display_name: 本地化
    dir: dimensions/language/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

## 选择输入

schema 必填，data、dimensions、codegen 可省略。schema/data 接受单路径或列表，目录分别递归读取 .cft/.cfd，新文件自动被发现；其他扩展名不会被当作数据。所有 CFT 共享声明，记录引用可以跨 CFD 文件，文件顺序不决定覆盖优先级。

```sh
coflow project add-input --project <项目> --kind data --path events/current.cfd --rules --if-revision <revision> --json
coflow project remove-input --project <项目> --kind data --path events/current.cfd --rules --if-revision <revision> --json
```

移除输入不删除文件，但其记录不再可用，剩余引用仍需有效。多项关联变更放进 apply files，见[校验与恢复](validation-and-recovery.md)。

## 维度目录

dimensions 的键为维度名，language 对应 @localized。dir 下 CFD 自动纳入输入，无需在 data 重复声明；尚未创建的维度目录视为空目录。变体从 CFD 块发现，不是 YAML 中的枚举清单。

覆盖可放在任意已纳入输入的 CFD，甚至与基础记录同文件。编辑器推荐覆盖路径保留基础文件相对项目根的目录，如 data/items.cfd 对应 dimensions/language/data/items.cfd；新增推荐覆盖需先配置 dir，已有覆盖写回原来源。

## 生成目录与路径

C# 目标 language 为 csharp，dir 必填，namespace 可省略（全局命名空间）。codegen 可以是单对象或列表；多个输出互不重叠，并与所有输入分离，业务扩展放在输出目录之外。

配置路径使用相对路径，项目外位置用 ../；无法表示为相对路径的跨盘位置会报错。维度目录彼此不重叠，并与 schema 输入分离。配置字段区分大小写，未知字段和重复字段产生诊断。

维度、namespace 或 codegen 等 YAML 调整可直接编辑配置，再执行 check --rules；需要产物时用 build。任务结束核对实际输入和所需输出，命令选择见[代码生成](generation.md)。
