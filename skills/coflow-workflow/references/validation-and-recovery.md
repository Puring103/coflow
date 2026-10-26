# 校验、诊断与源码恢复

## 选择检查范围

| 目标 | 命令 |
| --- | --- |
| 仅验证 CFT | `coflow schema check --project <项目> --json` |
| 验证结构、字段、引用与维度 | `coflow check --project <项目> --json` |
| 同时执行全部业务规则 | `coflow check --project <项目> --rules --json` |
| 定向解释规则 | `coflow check --project <项目> --rule <规则标识> --explain --json` |
| 验证候选变更 | `coflow check --project <项目> --input changes.json --rules --json` |

通过 `rule list` 获取规则标识。命名类型规则形如 `Item::ValidPrice`，顶层规则使用自身名称；匿名标识随源码位置变化，应重新查询。`--rule` 可重复，与 `--rules` 互斥。

业务断言失败按业务要求修正数据；若规则本身需改变，先确认规则目标。不能为了通过检查自行移除规则或编造默认值。求值错误、预算耗尽和未执行都需要处理，缩小范围定位后仍需完成任务要求的全部检查。

## 读诊断

先看 code、message、help，再用 record、field_path 和 location 定位。`coflow explain <code> --json` 提供触发原因、修复方向和查询建议。`--explain` 用于实际值及规则解释，`--details` 展开逐目标任务；默认诊断有数量上限。

常见处理顺序：配置与输入路径 → CFT 语法及类型 → CFD 语法和必填项 → 引用与覆盖 → 业务规则。字段错误也可能由声明变化引起，要结合 related 位置检查。行列计数和退出码见[协议](protocol.md)。

## 模型损坏时读取源码

```sh
coflow project inspect --project <项目> --json
coflow project files --project <项目> --json
coflow source read --project <项目> --file schema/main.cft --json
coflow source read --project <项目> --file data/items.cfd --json
```

project 命令尽量返回已知范围，source read 可用于读取已选中的源码。若配置本身无法加载、文件尚未纳入输入，直接读取对应本地配置或文件并保留诊断；不要循环调用依赖完整模型的 context/data 查询。

## 一次修复相关文件

把完整候选内容写在项目输入目录之外，避免候选 .cft/.cfd 被目录扫描成重复声明或记录。changes.json 与 candidates 目录并列：

```json
{
  "protocol_version": 1,
  "kind": "files",
  "changes": [
    { "op": "write", "file": "schema/main.cft", "content_file": "candidates/main.cft" },
    { "op": "write", "file": "data/items.cfd", "content_file": "candidates/items.cfd" }
  ]
}
```

```sh
coflow apply --project <项目> --input changes.json --rules --if-revision <revision> --json
```

只修数据时只列数据文件。候选必须包含完整文件内容，write 不是局部文本补丁。新增文件用 create，删除用 delete；输入调整用 add_input/remove_input，kind 为 schema/data。添加、改名字段或类型时同步处理所有基础记录、覆盖、模板及引用；结构设计需要相应 CFT 语法知识。

候选校验失败时原文件不发布，依据诊断修改候选后重试。无法通过 CLI 加载的配置修复可以直接编辑文件，随后 `check --rules` 验证。直接编辑不具有 apply 的事务保护。

## 冲突和 I/O 失败

conflict：重新读取目标、旧值、匹配数量及实际文件，再按当前业务目标形成新候选。不能仅替换 revision 或 expected_count。

I/O 失败：查看 written、written_files、restored_files、restoration_failed；若有恢复失败，先核对磁盘与外部修改。文件状态明确之前不声称回滚完成，也不盲目重放整批请求。

完成时说明修改范围、实际检查范围和仍存在的问题。`--dry-run` 成功说明预览通过；正式成功才说明写入完成。
