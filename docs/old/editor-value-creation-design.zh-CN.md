# CFD 编辑器值创建与默认物化设计

## 1. 目标

本设计统一新建记录、创建字段值、切换 `Option`、添加集合元素和创建多态 object 的行为。
编辑器优先采用 CFT 声明的字段默认值，其次采用类型默认值；无法自然产生合法值的必填引用保留为
可编辑的缺失状态。记录内部直接创建并展开 object，仅新建顶层记录使用完整弹窗。

语言编译阶段保证默认物化能够有限完成。运行时和编辑器共享同一套默认物化规则，不分别维护默认值
推断逻辑。

## 2. 概念

### 2.1 CFT 默认值

CFT 默认值是字段声明中显式给出的业务默认值：

```cft
type Reward {
  count: int = 10;
}
```

CFD 省略该字段时，模型构建使用 CFT 默认值。编辑器创建新值时也优先使用该值。

### 2.2 类型默认值

类型默认值是编辑器和 mutation 层创建新值时使用的确定性初始值。它不改变字段是否可以在 CFD 中省略；
只有 CFT 默认值和 `Option<T>` 的缺省语义允许字段在严格模型中省略。

| 类型 | 类型默认值 |
| --- | --- |
| `int` | `0` |
| `float` | `0.0` |
| `bool` | `false` |
| `string` | `""` |
| 普通 enum | 第一个声明的枚举项 |
| flag enum | `0` |
| `Option<T>` | `None` |
| `[T]` | `[]` |
| `{K: V}` | `{}` |
| 具体 object | 按本设计递归物化字段 |
| 抽象 object | 先选择可赋值的具体类型 |
| `&T` | 缺失引用 |

代码和协议使用 `TypeDefault` 表达该来源，不再使用 `TypeSeed`。

### 2.3 缺失状态

缺失状态属于编辑会话和 partial model，不是 `CfdValue` 的合法分支。必填引用没有 CFT 默认值时保持
缺失，不使用 `OptionNone` 伪装，也不自动选择第一条可用记录。

缺失引用具有明确的目标类型，可在表格、记录视图和新建记录弹窗中直接显示引用选择器。选择目标后，
字段变为普通引用值。未补齐的缺失引用产生诊断，并阻止 strict build、代码生成和发布；它不阻止编辑器
保存并继续修复 partial model。

## 3. 创建规则

### 3.1 优先级

创建一个字段时按以下顺序确定初始状态：

1. 字段存在 CFT 默认值时，物化该默认值。
2. 字段没有 CFT 默认值时，使用字段类型的类型默认值。
3. 类型没有合法值时，产生带类型信息的缺失状态。

object 创建对继承后的完整字段集合应用同一规则。集合新增元素没有字段级 CFT 默认值，直接使用元素
类型的类型默认值；当元素类型是抽象 object 时先选择具体类型。

### 3.2 具体 object

具体 object 直接创建并在当前位置展开，不打开独立的 object 填写弹窗。其字段按创建优先级递归物化。
必填引用可以保持缺失并在展开后的字段位置补齐。

### 3.3 抽象和多态 object

抽象类型本身不能实例化。创建以下值时，编辑器先显示可赋值具体类型的选择器：

- object 字段；
- `Option<Base>` 从 `None` 转为 `Some`；
- array 新增元素；
- dictionary 新增 value；
- 上述结构中的嵌套多态 object。

选择具体类型后立即按该类型创建值，不再打开字段填写弹窗。抽象类型、singleton、Host 以及不能在该
位置内嵌的类型不出现在候选项中。

多态 object 的标题行始终显示当前具体类型。具体类型右侧提供下拉按钮，候选项列出全部可赋值的具体
类型并标记当前项。非多态 object 只显示类型名称，不显示下拉按钮。

### 3.4 `Option<T>`

`Option<T>` 使用值右侧的加号和叉号切换状态：

- `None` 显示加号，点击后创建 `Some`；
- `Some(value)` 显示叉号，点击后变为 `None` 并删除内部值；
- `Some` 的内部值按创建优先级产生；
- `Option<&T>` 创建为包含缺失引用的 `Some`；
- `Option<ConcreteObject>` 直接创建并展开；
- `Option<AbstractObject>` 点击加号后先选择具体类型。

每一层嵌套 `Option` 独立显示自己的状态按钮。加号和叉号使用图标按钮，并提供“创建值”和“设为
None”的 tooltip 与无障碍名称。

### 3.5 引用

引用字段使用引用选择器。创建必填引用时允许进入缺失状态；创建可选引用时，外层 `Option` 决定其状态。

```text
&Item                 -> MissingRef(Item)
Option<&Item>          -> None
Some(Option<&Item>)    -> Some(None)
```

CFT 已声明引用默认值时直接使用该引用。引用目标不存在或类型不匹配时保留原始引用和诊断，不替换为其他
记录。编辑器不得根据记录顺序推断默认引用。

## 4. 递归类型与默认物化

### 4.1 类型结构递归

语言禁止必须立即展开的 object 包含环：

```cft
type A {
  child: A;
}

type Left {
  right: Right;
}

type Right {
  left: Left;
}
```

`Option`、array、dictionary 和引用提供有限终止状态，因此允许递归：

```cft
type Node {
  parent: Option<Node>;
  children: [Node];
  indexed: {string: Node};
  linked: Option<&Node>;
}
```

类型结构检查构建“必然 object 展开图”。只有解析别名后的直接 object 字段形成依赖边；`Option`、array、
dictionary 和引用终止该路径。直接或间接形成环时，schema 编译失败并报告完整字段路径。

### 4.2 默认值物化递归

合法的递归类型仍可能由 CFT 默认值触发无限物化：

```cft
type Node {
  child: Option<Node> = Some(Node {});
}
```

schema 编译阶段单独构建默认物化依赖。分析按实际默认表达式展开：

| 默认表达式 | 依赖行为 |
| --- | --- |
| `None` | 终止 |
| `Some(value)` | 继续分析 `value` |
| `[]` | 终止 |
| 非空 array | 分析每个元素 |
| 空 dictionary | 终止 |
| 非空 dictionary | 分析每个 value |
| object 字面量 | 分析显式字段，并分析被省略字段的 CFT 默认值 |
| 空 object 默认值 | 分析具体类型的全部字段默认物化 |
| 引用 | 终止 |

直接或间接的默认物化环在 schema 编译阶段报错。诊断包含从起始字段到回边的稳定路径，例如：

```text
default value materialization cycle: A.b -> B.a -> A.b
```

runtime 不再把合法 schema 的默认递归作为普通 mutation 错误处理。结构深度和节点预算继续作为输入规模
安全限制。

## 5. `Result<T, E>` 的字段限制

`Result<T, E>` 不允许作为自定义类型的数据字段，也不能通过 `Option`、array、dictionary 或类型别名嵌套
到数据字段中：

```cft
type Invalid {
  result: Result<int, string>;
  results: [Result<int, string>];
  optional: Option<Result<int, string>>;
}
```

字段类型检查解析别名并遍历数据容器；遇到函数类型时停止数据字段遍历，因此函数参数和返回类型中的
`Result` 保持合法：

```cft
type Service {
  run: fn(value: int) -> Result<int, string>;
}
```

`Result` 在常量、表达式、函数调用、传播、VM 和生成 API 中的语言值语义保持不变。

## 6. 编辑器交互

### 6.1 新建记录

新建顶层记录继续使用完整弹窗。弹窗包含 record key、具体类型和全部字段。字段初始状态由 runtime 返回
的统一创建草稿提供。

弹窗允许缺失引用存在并创建记录。写入时缺失字段保持省略，重新加载后的 partial model 展示相同缺失
状态与诊断。key、类型和已经提供的字段仍需满足相应的语法与类型约束。

### 6.2 已有记录

已有记录中的值在原位置创建：

- primitive 和 enum 直接使用类型默认值；
- 引用显示选择器；
- 具体 object 直接创建并展开；
- 抽象 object 先选择具体类型；
- `Option` 使用加号和叉号；
- array 和 dictionary 直接添加元素或条目。

记录内部不再打开完整 object 草稿弹窗，也不提供“创建默认类型”入口。界面命令统一使用“创建值”。

### 6.3 多态类型显示

object 卡片标题显示实际具体类型。多态值的类型右侧显示下拉按钮，支持在全部可赋值具体类型之间切换。
新建多态值和集合元素时复用相同的候选数据与显示组件。

具体类型切换作为一次 mutation 进入统一撤销/重做历史。切换过程中不产生可被其他视图观察到的中间
generation。

## 7. Runtime 与协议

### 7.1 默认物化器

`coflow-runtime` 保留一套 schema-guided 默认物化器，负责：

- CFT 默认值物化；
- 类型默认值创建；
- 具体 object 字段递归；
- 缺失字段分类；
- 多态候选校验；
- collection item 默认值创建。

`DefaultMaterialization` 的模式按最终调用语义收敛，删除仅为旧 object 弹窗服务的分支。语言层已经拒绝
默认物化环后，runtime 删除 `RecursiveObject` 形式的用户输入要求和依赖诊断字符串匹配。

### 7.2 创建草稿

创建草稿明确区分字段来源和状态：

```text
CreateFieldSource = SchemaDefault | TypeDefault | RequiredInput
CreateRequiredInput = Ref | AbstractObject
```

`RequiredInput` 表示字段尚无可写入值，具体缺失原因由 `CreateRequiredInput` 携带。协议直接携带目标引用
类型和多态候选，不允许前端根据诊断文本或错误码推断交互。

### 7.3 Partial 与 strict 边界

编辑会话继续使用 partial build。缺失必填引用、引用目标不存在以及引用类型不匹配均保留可定位字段和
诊断。strict build、代码生成与发布只接受没有 error 的完整模型。

结构化 writer 只写入草稿中实际存在的字段。缺失字段不序列化为 `None`、空 key 或其他占位值。

## 8. 实现计划

### 8.1 `coflow-language`

1. 在字段类型校验中增加数据字段 `Result` 限制，解析 alias 并穿透数据容器，函数签名作为边界。
2. 增加必然 object 展开图，在 schema 编译阶段拒绝直接和间接包含环。
3. 完整化默认物化依赖分析，覆盖 `Some`、非空集合、object 显式字段和省略字段。
4. 为字段类型限制、object 包含环和默认物化环提供稳定诊断码、source span 与路径。
5. 保持 schema 编译成功后 `ValueDependencyPlan` 无不可物化环的不变量。

主要涉及：

- `crates/coflow-language/src/schema/compiler/types.rs`
- `crates/coflow-language/src/schema/compiler/defaults.rs`
- `crates/coflow-language/src/schema/plans/value_dependencies.rs`
- `crates/coflow-language/src/diagnostics/`

### 8.2 `coflow-model`

1. 继续以字段缺失和非法引用状态构建 partial model。
2. 保证嵌套 object 中的缺失引用保留完整字段路径。
3. 删除针对 schema 默认物化环的模型层正常错误分支，由 schema 编译不变量替代。
4. strict build 保持对所有缺失和非法引用诊断的拒绝。

### 8.3 `coflow-runtime`

1. 将 `TypeSeed` 和相关注释、DTO 全量改为 `TypeDefault`。
2. 重构 `mutation/defaults.rs`，实现统一创建优先级和缺失引用结果。
3. 删除引用自动选择第一条记录的行为。
4. 删除 `RecursiveObject` required input 和依赖诊断文本匹配。
5. 为字段、object 和 collection item 提供同源的创建 API。
6. 多态创建 API 返回过滤后的具体类型候选；确认类型后返回具体 object 默认值。
7. writer 省略缺失字段，并由下一 generation 的 partial model 返回诊断。

### 8.4 编辑器 Core

1. 更新 wire DTO，明确传输 `SchemaDefault`、`TypeDefault`、缺失引用和具体类型候选。
2. 新建记录草稿允许缺失引用，不再要求 object 在前端组装为无缺失的 `CfdValue`。
3. 统一新建记录和已有记录的字段元数据、默认值与多态候选来源。
4. 类型切换、`Option` 切换和集合新增均转换为单次 runtime mutation。
5. 保持诊断导航精确到嵌套 object 中的缺失引用字段。

### 8.5 编辑器前端

1. 保留新建记录弹窗，并使其支持缺失引用选择器。
2. 删除记录内部的 object 草稿弹窗入口；`ObjectDraftHost` 仅保留为编辑器 lookup context。
3. 将 object 标题的实际类型显示与多态类型下拉合并为统一组件。
4. 具体 object 加号直接创建并展开；抽象 object 加号打开类型选择器。
5. `Option` 统一使用右侧加号和叉号切换 `None` / `Some`。
6. 缺失引用直接显示引用选择器，删除“填入默认值”和首条引用修复逻辑。
7. array 和 dictionary 新增多态元素时复用类型选择器。
8. 删除“创建默认类型”文案，统一使用“创建值”。

### 8.6 文档

1. 更新内部语言设计，记录 object 递归、默认物化递归和数据字段 `Result` 限制。
2. 更新编辑器容错加载设计，将“编辑种子”和首条引用修复改为类型默认值与缺失引用选择。
3. 更新公共 CFT、CFD 和数据模型参考，只描述用户可见语义，不写依赖图、DTO 或组件实现。

## 9. 测试与验收

### 9.1 语言测试

- 直接和间接的必然 object 包含环被拒绝；
- `Option<Node>`、`[Node]`、dictionary 和引用递归合法；
- `None`、空集合默认值终止递归；
- `Some(object)`、非空集合和省略 object 字段造成的默认物化环被拒绝；
- 数据字段及其容器内的 `Result` 被拒绝；
- 函数签名、常量和表达式中的 `Result` 保持合法；
- alias 不能绕过递归和 `Result` 字段检查。

### 9.2 Model 与 runtime 测试

- CFT 默认值优先于类型默认值；
- 所有类型默认值符合本设计；
- 必填引用创建为缺失，不选择第一条记录；
- 嵌套 object 缺失引用保留完整路径；
- partial model 接纳缺失引用，strict build 拒绝；
- 抽象字段和集合元素返回正确的具体类型候选；
- 具体 object 和 collection item 使用同一物化规则；
- 缺失字段不会写成 `None` 或伪造引用。

### 9.3 前端测试

- `None` 显示加号，`Some` 显示叉号；
- `Option<&T>` 可以创建为带缺失引用的 `Some`；
- 具体 object 直接创建并展开；
- 抽象字段和集合元素先显示类型选择器；
- object 标题显示实际类型，多态类型右侧显示下拉；
- 引用缺失可直接选择并修复；
- 记录内部不再打开 object 草稿弹窗；
- 类型切换、创建、删除均正确进入撤销/重做历史。

### 9.4 验证命令

正常开发必须从仓库根目录通过：

```powershell
cargo check --workspace
cargo test --workspace
```

编辑器协议和前端变化同时运行 frontend test 与 production build。实现期间不启动、停止或重启 CFD
编辑器。
