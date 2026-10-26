# 维护语言、平台与地区覆盖

先确认字段通过 CFT 的 `@localized` 绑定 language，或 `@dimension("region")` 绑定相应维度。普通字段需要调整类型声明后才能接受覆盖。

## 读取与核对

```sh
coflow dimension list --project <项目> --json
coflow dimension inspect --project <项目> --dimension language --json
coflow dimension variants --project <项目> --dimension language --json
coflow dimension get --project <项目> --type Item --id sword --field name --dimension language --variant ja --json
coflow dimension missing --project <项目> --dimension language --variant ja --json
```

get 的 `explicit` 表示字段是否写了覆盖，`variant_exists` 表示整个维度是否发现该变体，`override` 是显式值，`effective` 是类型化有效值，模板的求值文本另看 `rendered`。显式 None、空字符串、与基础值相同的赋值都算覆盖。missing 查缺少显式赋值的字段，按分页读完后再报告翻译完整性。

保存响应 revision。批量翻译前取得基础值、相关语境和已有译文；语言或业务措辞不明确时向用户确认。

## 写入与清除

```sh
coflow dimension set --project <项目> --type Item --id sword --field name --dimension language --variant ja --value-file translation.cfd-value --rules --if-revision <revision> --json
coflow dimension clear --project <项目> --type Item --id sword --field name --dimension language --variant ja --rules --if-revision <revision> --json
```

translation.cfd-value 的内容是带引号的 CFD 字符串，例如 `"剣"`。clear 删除显式覆盖，读取恢复基础表达式；不保证该变体从整个项目消失。

多项用 apply 的 `set_dimension_value` / `clear_dimension_value`，参考[dimension-changes.json](../examples/dimension-changes.json)。示例要求 Item.name 绑定 language，Item.price 绑定 region，两个记录 sword、shield 已存在；写入文件 data/items.cfd 已纳入输入。

set_dimension_value 可带 path，相对 field 指定的顶层字段定位内部值；保存仍是完整顶层字段覆盖。对对象、数组、字典的直接 CFD 覆盖不会逐项合并。路径结构使用 [CLI 协议](protocol.md)，与插件路径不同。

## 文件选择

已有覆盖修改后写回原来源。新增覆盖可显式给 `--file` 或请求的 file，文件应属于数据输入；基础与覆盖可放在同一 CFD。使用维度专用目录时配置 `dimensions.<名称>.dir`，目录会自动纳入输入，见[项目配置](project-config.md)。

同一维度、变体、记录、字段只能有一处覆盖。变体从覆盖块发现，`language[ja] {}` 这样的空块也声明变体。文件顺序不提供优先级。

## 回退与模板

已知维度中未知变体或未填写字段读取基础表达式。模板在当前上下文重新求值：基础 label 为 `f"购买 {self.name}"`，日语只覆盖 name 时，日语 label 可以显示日语 name，但 label 仍没有显式覆盖。覆盖状态看 explicit，普通值看 effective，模板文本看 rendered。

```sh
coflow dimension diff --project <项目> --dimension language --from-base --to ja --json
coflow stats dimension --project <项目> --dimension language --variant ja --json
```

`base` 可以是普通变体名，基础视角用 from-base/to-base 选择。完成时区分“翻译写入成功”“还有缺失覆盖”和“回退显示正常”，不能互相替代。
