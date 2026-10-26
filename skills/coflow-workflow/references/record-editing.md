# 新增和维护记录

先读取目标类型与当前值，保存 revision。以下示例假定 Item 有 name、price、next 等相应字段；命令补上 `--project <项目>`。

## 改值、恢复默认与清空

```sh
coflow data set --type Item --id sword --path price --value-cfd 100 --expected-cfd 90 --rules --if-revision <revision> --json
coflow data unset --type Item --id sword --path note --rules --if-revision <revision> --json
```

set 建立显式赋值；unset 移除赋值并重新应用默认值或必填检查。要将 Option 明确置空，使用 set 的 `--value-cfd None`。unset 与设为 None 不等价。

`--expected-cfd` 按目标字段类型解析，并比较补齐默认值后的当前基础有效值；模板比较 f 源表达式。`--if-revision` 保护项目读取修订，两者可同时使用。

字符串和嵌套值可放在 UTF-8 文件，例如文件内容为 `"长剑"`，通过 `--value-file name.cfd-value` 传入，避免 Shell 转义。路径穿过 None 时，先设置包含 Some 的父值；不能把不可达位置当成已有字段直接改。

## 创建

```sh
coflow data draft --type Item --id new_sword --json
coflow describe data.create --schema input --json
```

draft 的 `complete` 和 `required_inputs` 指出还需提供什么。类型初值不等于业务默认值；需要用户决定的必填业务值应取得实际输入。

`data create --input` 是字段名到输入值的对象，不是整个 apply 变更集：

```json
{
  "name": { "kind": "expression", "source": "\"长剑\"" },
  "price": { "kind": "expression", "source": "100" }
}
```

```sh
coflow data create --type Item --id new_sword --file data/items.cfd --input fields.json --rules --if-revision <revision> --json
```

目标文件需属于项目数据输入；目录输入会自动发现其中的 CFD。多条相互引用的新记录用同一份[批量变更](batch-editing.md)，引用在全部操作后验证。

## 改名、迁移与删除

| 目标 | 命令核心参数 | 影响 |
| --- | --- | --- |
| 改 ID | `data rename --type Item --id sword --new-id iron_sword` | 更新已识别引用、覆盖目标和稳定枚举映射 |
| 换来源文件 | `data transfer --type Item --id sword --to-file data/weapons.cfd` | 基础记录迁移，已有覆盖仍保留来源文件 |
| 删除记录 | `data delete --type Item --id obsolete` | 删除记录及覆盖；剩余引用必须仍有效 |

上述写入同样携带 `--rules --if-revision <revision> --json`。需要检查补丁时加 dry-run。改名只处理 Coflow 已识别的引用，应用源码中的字符串 ID 等外部使用需要结合用户范围另外核对。

若要删除一个有入向引用的记录，先确定这些引用应改向哪里或是否同时删除，将关联操作放在一次 apply 中；不要靠移除校验使候选通过。

## 完成与异常

退出码 0、成功状态以及写入结果说明修改已完成，成功结果已包含所选规则校验。冲突后重读当前目标与业务范围；失败时根据[校验与恢复](validation-and-recovery.md)处理。需要向用户展示的值可定向读取，无需重复整项目检查。
