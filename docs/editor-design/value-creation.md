# 编辑器值创建与默认物化

## 值来源

字段创建优先使用 CFT 显式默认值，其次使用类型的编辑初始值；必填引用与需要选择具体类型的字段保留待输入状态。编辑初始值用于创建交互，不改变 CFD 的字段省略规则。字段只有显式默认值时才可省略；`Option<T>` 只决定值是否可为空。

runtime 的默认物化器负责记录草稿、具体 data 对象及集合元素创建，字段草稿以 `SchemaDefault`、`TypeDefault`、`RequiredInput` 标记来源。`RequiredInput` 携带引用目标、抽象对象候选或无法物化的具体原因，前端据此提供输入入口。

| 类型 | 编辑初始值 |
| --- | --- |
| `int`、`float` | `0`、`0.0` |
| `bool` | `false` |
| `string` | `""` |
| 普通 enum、flag enum | 第一个声明成员的值；flag 值保存数值组合 |
| `Option<T>` | `None` |
| 数组、字典 | 空集合 |
| 具体 data | 按继承后的完整字段集合递归创建 |
| 抽象 data | 用户选择可赋值的具体 data 类型 |
| `&Table` | 用户选择引用目标 |

字符串编辑器使用 `T` 与 `ƒ` 切换文本和模板模式；模板模式写入显式 `f"..."` 源表达式。可编辑区域直接显示作者输入的正文，模板的渲染结果用于只读值展示。

引用没有隐式选中记录。已有默认引用保留指定目标；目标不存在或类型不匹配时提供诊断与修复入口。

## 缺失与创建交互

缺失字段是 partial model 的状态，不是 `CfdValue` 的合法值分支。新建记录弹窗可以保存带缺失必填引用的草稿，writer 只写实际提供的字段；重载后保留记录坐标、缺失状态和诊断。严格模型、代码生成与发布要求消除这些错误。

新建顶层记录使用包含记录 id、具体类型与字段的弹窗。已有记录在原位置创建字段、展开 data 对象、添加集合元素。抽象或多态 data 提供具体类型选择；已有多态对象显示实际类型并允许切换，切换作为一次 mutation 进入历史。

每层 `Option` 独立提供创建和清除按钮。创建中间可选层得到 `Some(None)`；创建最内层时按内部类型生成值。可选引用通过引用选择器选择目标后形成 `Some(reference)`，具体 data 直接创建，抽象 data 先选择类型。

缺失必填引用直接提供引用选择器；其他可生成初始值的缺失字段显示 `Missing` 和创建入口。引用目标未明确时不写入空 id、`None` 或任意第一条记录。

## 递归边界

记录引用允许直接或间接成环，因为记录身份先于字段连接建立。例如下面是合法结构：

```cft
table Node {
  next: &Node;
}
```

内联 data 的必填包含关系必须有限。直接或间接的必填 data 包含环在 schema 编译时拒绝：

```cft
data Node {
  child: Node;
}
```

`Option`、数组、字典及记录引用终止必然展开路径。可选递归 data 可以存在，但显式默认值仍须有限物化。下面的默认值重新触发自身默认值，因此编译失败：

```cft
data Node {
  child: Option<Node> = Some(Node {});
}
```

显式终止的 `Some(Node { child: None })`、`None` 和空集合可以有限物化。分析覆盖显式字段、被省略字段的默认值、非空集合及 `Some`，并报告完整依赖路径。引用仅连接记录身份，不展开目标记录的默认值。

## 模块边界

语言层负责必填包含环与默认值物化环；model 负责严格与部分模型的字段状态；runtime 负责 schema 引导的创建与 mutation；编辑器 core 提供草稿和字段元数据，前端负责选择和展示。结构化写入、重载与 generation 发布沿用同一事务和历史机制。

实现与现有回归用例：

- [mutation/defaults.rs](../../crates/coflow-runtime/src/mutation/defaults.rs)：字段草稿与默认值物化。
- [value_creation_schema.rs](../../crates/coflow-language/tests/value_creation_schema.rs)：包含环和有限默认值。
- [DataCard.tsx](../../editors/cfd-editor/frontend/src/features/record-editing/components/DataCard.tsx)：可选层、对象类型选择和缺失修复。
- [容错加载](tolerant-loading.md)：partial model 与严格消费者边界。
