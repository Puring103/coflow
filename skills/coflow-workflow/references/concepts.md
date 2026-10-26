# 维护数据前需要知道的语义

`coflow.yaml` 选择 CFT 类型、CFD 数据和生成目标。CFT 定义结构、默认值及只读业务规则；CFD 保存记录与维度覆盖。应用使用生成代码与 CFD，CFT 的 check 由 CLI 或编辑器执行。

## 身份与值

| 概念 | 操作含义 |
| --- | --- |
| `table Item` | 多条独立记录，操作目标为实际类型与 ID |
| `singleton Settings` | 恰好一条记录，ID 为 `Settings` |
| `data Stats` | 属于字段的内联对象，通过所属记录的字段路径修改 |
| `&Item` | 记录引用；改名用记录命令更新已识别引用 |
| `Option<T>` | 可空；只有字段声明了默认值才允许省略 |

同一继承树中的记录 ID 唯一，无关类型树可以使用同一个 ID。table 与 singleton 不声明自己的 id 字段；data 可有普通业务 id。

## 如何提供字段值

先用 `schema inspect --type Item --field <字段> --json` 或 `context --type Item --json` 获取字段类型、默认值和候选。CLI 的 `--value-cfd`、`--value-file` 和 JSON expression 均使用 CFD 值语法：

| 字段类型 | CFD 表达式 |
| --- | --- |
| `int` / `long` | `100` / `4294967296`，按 32 / 64 位有符号范围检查 |
| `float` / `double` | `1.5`，有限浮点；float 按 32 位精度舍入 |
| `string` | `"铁剑"`；模板为 `f"{self.name}：{self.price}"` |
| `bool` | `true` |
| 枚举 | `Quality::Rare`；位枚举可用 `Permission::Read \| Permission::Write` |
| `&Item` | `&Item::sword`，已知目标类型时可写 `&sword` |
| `Option<T>` | `None` 或 `Some(值)`；嵌套 Option 保留每层状态 |
| `[T]` | `[1, 2]` |
| `{string: T}` | `{ "title": "物品" }` |
| data 类型 | `Stats { hp: 100 }`；声明类型确定实际类型时可省略类型名 |

Shell 会先处理引号。复杂字符串可写入 UTF-8 文件，再用 `--value-file`；JSON 请求中的 expression.source 是包含 CFD 引号的字符串。结构化 model 值见[协议](protocol.md)。

## 默认值、清空与覆盖

- `set` 建立显式赋值；`unset` 删除赋值，再应用 CFT 默认值。无默认值的必填字段不能通过 unset 变成合法缺失。
- `None`、空字符串、空数组、空字典各自是值；它们与没有赋值不同。
- `Option<string>` 无默认值时必须填，`Option<string> = None` 才能省略；裸值在 Option 位置表示一层 Some。
- `data get --view both` 同时看源码与有效值；不要将默认值来源误认为已写入源码。
- 普通 `"{name}"` 保持原文，只有 `f"..."` 插值。修改模板应保留其源表达式，不能用渲染后的文本覆盖。
- 维度覆盖替换整个顶层字段；未覆盖时在所选上下文求值基础表达式。翻译是否填写看显式覆盖，见[维度维护](dimensions.md)。

CFD 记录为 `sword: Item { name: "铁剑", price: 90 }`，字段用逗号，`#` 为注释。文件中的所有引用目标需要一起纳入项目；文件顺序不提供覆盖优先级。
