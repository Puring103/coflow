# CFT 与 CFD 源码格式化设计

> 状态：当前内部设计契约
>
> 范围：CFT/CFD 共享格式化核心、规范输出、宿主接入与项目级文件发现。

格式化核心由 `coflow-format::format_cft` 和 `coflow-format::format_cfd` 持有。`coflow-language`
提供保留 trivia 与 byte span 的共享无损 token 流，格式化器据此识别字符串、注释、字面量、符号和
结构边界。CLI 与 LSP 直接复用格式化核心，CFD 编辑器通过内嵌 LSP 请求格式化。公开文档只说明命令
用法和基本缩进约定；完整格式契约保留在本目录。

## 1. 边界与不变量

格式化器是消费无损 token 流的空白规范化器，不是完整 AST pretty printer，也不是 parser 或 checker。
它只改变 trivia、换行和结构布局，不重排或改写非 trivia token，不补默认值，也不执行 schema-guided
类型检查。

必须保持以下不变量：

- 对有效输入保留 token、注释内容和声明/值顺序。
- 对可识别的编辑中未完整文本尽可能产生稳定布局，不要求先通过语义检查。
- 输出幂等；对格式化结果再次调用同一 formatter 不产生变化。
- 规范输出使用 LF，并以一个换行符结束。
- parser、runtime 和 checker 始终是语法、类型及数据有效性的权威。

## 2. 通用排版

- 每一级缩进为 2 个空格，不使用制表符。
- 行尾不保留空格；连续空行最多保留一个。
- `#` 注释内容保持不变，缩进跟随所在结构。
- 二元运算符、`=`、`->` 两侧使用规范空格；`:` 和逗号后使用一个空格。
- `::`、字段访问 `.`、一元运算符和泛型尖括号内侧不添加空格。
- 顶层声明或记录之间使用一个空行。

## 3. CFT 布局

type、enum、check 和函数体的左花括号与头部放在同一行，结构成员各占一行：

```cft
@label("Item")
type Item : Entity {
  name: string;
  tags: [string] = [];

  check {
    name != "";
  }
}
```

- 字段、const、类型别名和 check 条件以分号结束。
- enum 变体以逗号分隔；多项 enum 展开为每行一个变体。
- 注解单独占行并紧贴它修饰的声明；字段注解组前保留一个空行。
- 多行泛型、函数参数和续行相对所属行再缩进一级。
- 短数组、字典和对象默认值可以保持单行；结构性 type/check 块始终展开。

```cft
type Handler = fn(
  value: int,
  fallback: fn(int) -> int
) -> Result<
  int,
  string
>;
```

## 4. CFD 布局

顶层记录、group 记录和带 type marker 的多态对象使用展开块。字段每行一个并保留尾逗号：

```cfd
Item {
  sword {
    name: "Sword",
    tags: [weapon, melee],
  }

  shield: Equipment {
    name: "Shield",
  }
}
```

- 记录写作 `key: Type {`，冒号后一个空格。
- group 中相邻记录之间保留一个空行。
- 展开块的右花括号单独占行；若它是字段值，逗号跟在右花括号后。
- 数组内的多态对象展开后，数组右方括号单独对齐。
- 简单数组、字典和无 type marker 的内联对象允许保持单行。
- 函数字段签名尽量合并到同一逻辑行；多行签名和函数体按 2 空格层级缩进。
- 函数体中手工保留的单个空行不会被删除。

## 5. 宿主接入

单文件宿主直接调用 `coflow-format`。LSP 从当前 document snapshot 生成格式化结果，并负责转换为互不
重叠且 UTF-16 位置正确的 `TextEdit`；规范输入返回空 edits。宿主可以在编辑中的未完整文本上调用
formatter，但不能把“格式化成功”解释为文件已经通过解析或检查，也不得追加会改变规范输出的私有
排版步骤。

runtime writer 负责 schema-guided CFD 局部写入，直接生成规范局部文本，不调用通用 formatter，避免
单字段修改引发整文件重排。writer 测试使用 `coflow-format` 验证完整输出已满足规范格式。

项目级 `coflow format [CONFIG_OR_DIR] [--check]` 遵循以下发现与写入规则：

- 只处理目标 `coflow.yaml` 配置的 schema 和 data 路径，以及小写扩展名 `.cft` / `.cfd` 文件。
- 重叠配置路径解析为真实路径后去重，每个文件最多处理一次。
- 跳过 `dimensions.*.out_dir` 下由 Coflow 管理的生成文件。
- 默认模式只替换发生变化的文件，并使用同目录临时文件完成原子替换。
- `--check` 不写文件；任一文件与规范输出不同即返回非零状态并列出文件。

项目提交前，格式检查仍需与相应的 `coflow cft check` / `coflow check` 配合使用。
