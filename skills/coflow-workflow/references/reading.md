# 查询、筛选和核对范围

类型未知先读[项目发现](discovery.md)。以下 Item、字段和 ID 都需要替换为项目中的真实目标；每条项目命令带 `--project <项目>`。

## 按问题选择查询

```sh
coflow context --type Item --fields name,price --examples 3 --json
coflow data get --type Item --id sword --fields name,price --view both --json
coflow data list --type Item --where 'price < 100' --fields name,price --limit 20 --json
coflow data search --text 治疗 --limit 20 --json
coflow data refs --type Item --id sword --direction incoming --json
coflow schema refs --symbol Item.price --json
```

`context` 返回相关声明、规则、样例和引用候选，不代表全部记录。用 `--examples`、`--ref-limit`、`--depth`、`--budget` 控制读取量；budget 是序列化字符预算。查全部目标用 list 并翻页。

`data refs` 查记录引用，删除或改名之前可用于了解影响；`schema refs` 查类型或字段的声明使用位置。继承树查询需要子类型时显式使用 `--include-derived`，返回记录的实际类型用于后续写入。

## 查询条件

文本路径可写 `stats.hp`、`tags[0]`、`labels["a.b"]`。`--where` 是 CLI 的类型化筛选语言，支持比较、布尔组合及 `contains`、`is_some`、`is_none`、`is_reachable`，不执行任意 CFT check 表达式或算术赋值。

```sh
coflow data list --type Item --where 'price >= 0 && price < 100' --sort price --json
coflow data list --type Item --where 'contains(tags, "weapon")' --json
coflow data list --type Item --where 'is_none(next)' --json
```

`--sort` 为逗号分隔路径，路径前加 `-` 表示降序。`--dimension language=ja` 选择查询上下文，可重复组合不同维度。筛选和写入涉及维度时，确认用户要修改基础字段还是覆盖。

## 有效值与来源

`--view source|effective|both` 选择显式赋值、有效值或两者。`rendered` 适合展示，`fields` 中的类型化值适合写回；来源与模板表达式按需读取。

`origin` 区分 `explicit_base`、`schema_default`、`dimension_override`。显式 None 仍有来源；穿过 None 的路径是 `unreachable` 且 `reachable: false`，不能当成可写的空字段。

## 翻页与完整性

列表默认每页 50 项。按 `result.next_cursor` 继续，保持查询条件不变；可调整 limit。`total` 是匹配总数，当前页 items 长度不是总数。

- `truncated` / `next_cursor`：输出还有后续内容。
- `data_scope.complete`：输入数据是否全部可用。
- `revision`：这次读取对应的项目修订。

数据加载不完整时不能据当前结果断言“没有其他引用”或“全部匹配”。项目改变导致旧游标 conflict 时去掉 cursor，从第一页重查。

## 统计

```sh
coflow stats records --group-by type --json
coflow stats field --type Item --field price --metrics count,min,max,mean --json
coflow stats dimension --dimension language --variant ja --group-by type --json
```

统计的计算范围是全部匹配记录，limit 只限制输出行。整数总和用精确十进制字符串；Option、不可达路径和空集合有单独统计口径，结合结果解释。维度覆盖率统计显式覆盖，不把回退有效值计为已翻译。

交付查询结论时注明筛选范围、是否包含派生类型，以及尚未读取或加载失败的范围。批量写入前记录查询 revision 与核实后的 total。
