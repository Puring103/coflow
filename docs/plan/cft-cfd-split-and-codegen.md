# CFT/CFD 拆分与目标语言代码生成

本文记录从单一混合 CFC 文件格式硬切换到类型文件与数据文件分离后的设计方向，并明确目标语言类型生成、运行时加载与工具编辑的边界。

## 动机

当前 CFC 模型允许类型定义和数据定义写在同一个源文件里。这个设计对小示例很方便，但在大型项目中会带来几个问题：

- 数据和定义没有分离，工具很难只安全地修改数据而不碰 schema；
- 如果要求每种目标语言都直接解析和校验源文件，loader 会变得过于复杂；
- 应用领域不够明确，格式同时像 schema 语言、数据语言和通用配置语言。

新的方向是把 Coflow 配置系统定位为面向游戏数据、编辑器工具、仿真数据和复杂对象图内容管线的强类型内容作者格式。

## 完整 Coflow 组件

完整 Coflow 系统包含四个主要组件：

1. `cft` 类型定义语言：用于定义类型、枚举、字段约束和类型级校验规则。
2. `cfd` 图数据语言：用于保存带 identity、引用关系和名义类型信息的图数据。
3. `cfs` 轻量级嵌入式脚本编程语言：用于在宿主程序中编写运行时逻辑，可以直接 `import` `.cft` 和 `.cfd` 文件。
4. Coflow Studio：面向 `.cfd` 数据的编辑器，支持使用多种视图编辑同一份图数据。

本文主要讨论 `.cft` / `.cfd` 的硬拆分、数据加载和目标语言代码生成边界。`.cfs` 和 Coflow Studio 需要与这套边界保持一致，但它们的完整语言设计和产品设计应分别展开。

## 文件类型

定义和数据使用不同后缀：

- `.cft`：Coflow Type File，类型定义文件；
- `.cfd`：Coflow Data File，数据文件；
- `.cfs`：Coflow Script File，轻量级嵌入式脚本文件；
- `.cfc`：移除，不再作为合法输入格式。

这是破坏性切换。不保留 mixed 或 legacy 兼容模式。

## CFT 规则

`.cft` 文件只定义 schema。

允许：

- `use` 导入其他 `.cft` 文件；
- `type` 定义；
- `enum` 定义；
- `type` 内部的 `check` 块。

禁止：

- 顶层数据节点；
- 顶层 `check` 块；
- 导入 `.cfd` 文件。

示例：

```cfc
// schema/item.cft
enum Rarity {
  Common = 1;
  Rare = 2;
}

type Item {
  id: string;
  name: string;
  rarity: Rarity;
  price: int = 0;

  check {
    price >= 0;
  }
}
```

## CFD 规则

`.cfd` 文件只定义数据。

允许：

- `use` 导入 `.cft` 文件以使用类型；
- `use` 导入 `.cfd` 文件以使用其他数据节点；
- 顶层有类型标注的数据节点；
- 顶层 `check` 块。

禁止：

- `type` 定义；
- `enum` 定义。

示例：

```cfc
// data/items.cfd
use "../schema/item.cft" as schema;

potion: schema.Item = {
  id = "potion";
  name = "Potion";
  rarity = schema.Rarity.Common;
  price = 10;
};

check {
  potion.id == "potion";
}
```

## Import 方向

导入图必须强制定义和数据的分层：

- `.cft -> .cft`：允许；
- `.cft -> .cfd`：禁止；
- `.cfd -> .cft`：允许；
- `.cfd -> .cfd`：允许。

这样 schema 文件不会依赖数据文件，但数据文件仍然可以引用类型，也可以跨模块共享数据节点。

## 产品边界

旧定位是：

> CFC 是自校验强类型配置语言。

新定位应当是：

> Coflow 配置系统由 `.cft` 类型定义文件和 `.cfd` 数据文件组成。`.cft` 是编译期 schema 输入，`.cfd` 是可编辑的数据输入。工具和目标语言运行时可以在生成的 schema metadata 帮助下直接加载 `.cfd`。

系统边界因此变成：

- `.cft` 文件定义类型和校验规则；
- `.cfd` 文件保存数据，允许外部编辑器修改；
- 目标语言生成器消费 `.cft`，产出目标语言类型定义和 schema metadata；
- 目标语言库结合生成的 schema metadata 与 `.cfd` 数据，将其 materialize 成强类型对象。

这个边界避免把 `.cft` 定位成运行时格式，同时保留 `.cfd` 被编辑器和应用运行时直接使用的能力。

## 设计约束

以下约束应作为 `.cft` / `.cfd` / Studio / 目标语言运行时的共同边界。

### 样式与数据分离

`.cfd` 只保存图数据，不保存 Studio UI 状态。

不应写入 `.cfd`：

- 窗口布局；
- 表格列宽；
- 当前选中节点；
- 当前折叠状态；
- 颜色主题；
- 最近打开记录；
- 只对某个本地编辑器有意义的临时状态。

Studio 的本地状态应放在本地配置目录中，并默认不提交到版本库。例如：

```text
.coflow/studio/
  layout.local.json
  recent.local.json
  view-state.local.json
```

如果未来需要团队共享视图配置，也不应塞进 `.cfd`。可以单独设计共享视图文件，例如 `.cfview`，但这不是 `.cfd` 的职责。

### CFT 元数据边界

`.cft` 可以包含元数据，但元数据应服务于数据语义、编辑辅助和代码生成，不应成为 UI 样式系统。

适合放入 `.cft` 的元数据：

- `displayName`；
- `description`；
- `category` / `tags`；
- `deprecated`；
- editor hint，例如 `multiline`、`range`、`assetRef`；
- id 字段标记；
- ref / index 标记；
- 代码生成提示。

不适合放入 `.cft` 的元数据：

- Studio 窗口布局；
- 表格列宽；
- 主题颜色；
- 当前折叠状态；
- 当前选中节点；
- 只影响某个具体 UI 的样式信息。

原则是：`.cft` 元数据可以帮助所有工具理解数据语义，但不能绑定某个 UI 的具体表现。

### CFD 快速加载

`.cfd` 应保持纯数据，以保证目标语言运行时可以快速加载。

`.cfd` 不应包含或依赖：

- 任意表达式求值；
- 函数调用；
- 条件逻辑；
- 计算字段；
- schema 定义；
- 运行时脚本能力。

目标语言运行时加载 `.cfd` 的理想流程是：

```text
读取 .cfd
-> 使用生成的 schema metadata 解释字段
-> 创建对象
-> 解析引用
-> 填充默认值
-> 建立索引
```

复杂校验应属于工具链，而不是运行时加载库。

### Studio 只编辑 CFD

Coflow Studio 的默认职责是编辑 `.cfd` 数据。

`.cft` 由程序员或数据系统设计者维护，Studio 将 `.cft` 当成只读 schema 输入，用它驱动：

- 字段结构；
- 类型提示；
- 默认值；
- 引用选择器；
- enum 选择器；
- 校验诊断；
- 多视图编辑。

Studio 不应在普通数据编辑流程中修改 `.cft`。如果未来需要 schema 编辑能力，应作为独立的 schema designer 模式设计，不应混入 `.cfd` 数据编辑器。

### Diff 友好

`.cft` 和 `.cfd` 都必须是 diff 友好的源文件格式，适合代码审查和版本管理。

格式设计和工具写回应遵守：

- 源格式使用文本，不把二进制作为真源；
- 提供稳定 `fmt`；
- 一个顶层数据节点保持一个稳定 block；
- 字段顺序稳定；
- 不写入生成时间、随机 id、机器路径等高噪音内容；
- 引用使用稳定名字或 id，不使用行号或易变化数组下标；
- 大数组和大字典支持多行格式；
- 工具写回时尽量保留注释、空行和声明顺序；
- 保存 `.cfd` 时避免无意义重排整个文件。

Diff 友好是 Studio、CLI 和外部编辑器共同遵守的格式契约。

## 运行时加载边界

目标语言不应该实现完整 `.cft` 源文件 loader。`.cft` 是代码生成和校验阶段的编译期输入，不是运行时输入。

目标语言运行时可以直接加载 `.cfd` 数据文件，但必须依赖生成的 schema metadata 和共享的目标语言运行时库。运行时库应理解纯数据子集和生成的类型元数据，不应理解类型定义语言。

不要生成：

- `.cft` parser；
- import resolver；
- symbol table builder；
- 完整结构校验器；
- 默认值填充语义；
- 循环分析；
- `check` 执行器。

这些语义属于参考实现和 CLI。

直接数据加载管线：

```text
.cft schema
  -> cfc gen <target>
  -> 目标语言类型定义 + schema metadata

.cfd data
  -> 外部编辑器修改数据
  -> 目标语言运行时库结合生成的 schema metadata 加载数据
```

构建流程仍然可以在打包前运行 `cfc check`。运行时加载只做保护反序列化所需的检查，例如格式错误、未知字段、缺少必填字段、非法枚举值和断开的引用。

也可以保留可选的导出管线，供需要打包图数据的项目使用：

```text
.cft + .cfd source
  -> cfc check
  -> cfc export game.cfgraph
  -> 目标语言运行时加载 cfgraph
```

关键边界是：目标语言运行时可以加载 `.cfd` 或导出的图数据，但永远不加载 `.cft`。

## Check 边界

`check` 只属于 Coflow 工具链，不进入目标语言数据加载库。

`cfc check` 负责：

- 解析 `.cft` / `.cfd`；
- 执行结构校验；
- 执行 `type` 内 `check`；
- 执行顶层 `check`；
- 输出 diagnostics；
- 服务于 Studio 保存前、构建前和 CI。

目标语言加载库负责：

- 读取 `.cfd` 或导出的图数据；
- 使用生成的 schema metadata；
- 做最低限度反序列化保护；
- materialize 成目标语言对象；
- 不执行 `check`。

目标语言加载库可以保留的必要检查：

- 文件格式是否能解析；
- 字段是否存在；
- 字段类型是否能转换；
- enum 值是否合法；
- ref 是否能解析；
- union 分支是否合法。

目标语言加载库不做：

- `type` 内 `check`；
- 顶层 `check`；
- `all` / `any` / `none`；
- `min` / `max` / `unique`；
- 表达式求值；
- 跨节点业务约束。

推荐流程是：

```text
Studio 保存前 -> cfc check
构建/打包前 -> cfc check
CI -> cfc check
运行时 -> 快速加载已校验数据
```

如果需要防止运行时加载未经校验的数据，可以在构建产物中记录：

- `.cfd` hash；
- `.cft` schema hash；
- `cfc` version；
- 最近一次 check 的摘要。

运行时只检查 hash、schema version 或 check 摘要是否匹配，不重新执行 `check`。

## CFS 边界

`.cfs` 是完整 Coflow 系统中的脚本语言组件。它可以直接 `import` `.cft` 和 `.cfd` 文件，因为脚本运行时需要同时理解类型定义和图数据。

这和目标语言运行时的边界不同：

- 目标语言运行时不加载 `.cft`，只使用生成的 schema metadata；
- `.cfs` 运行时可以加载 `.cft`，因为它属于 Coflow 自身语言系统；
- `.cfs` 可以围绕 `.cfd` 对象图编写运行时逻辑；
- `.cfd` 仍然保持纯数据，不因为被 `.cfs` import 而获得脚本能力或副作用。

因此，`.cfs` 是 Coflow 内部脚本层，不应被当作目标语言 loader 的替代品。

## Coflow Studio 边界

Coflow Studio 是 `.cfd` 图数据的编辑器。它应以 `.cft` 提供的 schema 和 `.cfd` 提供的数据为输入，支持多种视图编辑同一份图数据。

Studio 的核心边界：

- 主要编辑对象是 `.cfd`；
- `.cft` 用于驱动字段结构、类型提示、默认值、校验和视图生成；
- 同一份 `.cfd` 数据可以有表格、树形、对象详情、关系图、表单等多种视图；
- Studio 写回 `.cfd`，不把数据保存成私有项目格式；
- Studio 可以调用 `cfc check` 或共享校验库，在保存前报告结构错误和约束错误；
- Studio 不应绕过 `.cfd` 源格式成为新的数据真源。

这保证外部编辑器、版本管理、CLI、目标语言运行时和 Studio 都围绕同一份 `.cfd` 数据工作。

## 导出格式

导出产物是稳定的图表示。第一版可以使用 JSON，未来可以增加二进制格式。

导出格式必须保留：

- 对象 identity；
- 引用关系；
- 名义类型名；
- union wrapper 信息；
- enum 元数据；
- 顶层数据节点名。

必要概念包括：

- `$id`：图对象 identity；
- `$ref`：对象引用；
- `$type`：名义对象类型；
- `$union`：union alias wrapper。

具体格式留给单独的导出格式设计文档。

## 目标语言类型生成

目标语言类型定义应当来自 schema reflection 或 `cfc schema`，而不是由目标语言重新解析 `.cft` 文件。

生成物必须同时包含类型定义和机器可读的 schema metadata。只有 class/record/interface 不够，因为直接加载 `.cfd` 时，运行时库还需要字段类型、默认值、引用行为、union 映射、enum 映射和顶层数据节点结构。

类型生成应覆盖：

- 带显式整数值的 enum；
- object type 对应的 class/record/struct；
- nullable 字段；
- 数组/list；
- 字典/map；
- 名义类型引用；
- union alias；
- 目标语言能安全表达的默认值。

C# 生成示例：

```csharp
public enum Rarity
{
    Common = 1,
    Rare = 2,
}

public sealed class Item
{
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
    public Rarity Rarity { get; set; }
    public int Price { get; set; } = 0;
}
```

第一版中，`check` 块保持为构建期校验。之后可以考虑生成运行时 validator，但这不是第一个目标语言生成器的必要能力。

## 目标语言 Loader 生成

Loader 应拆成稳定的运行时库和项目专属的生成代码。

运行时库职责：

- 读取 `.cfd` 数据文件，也可选支持导出的图文件；
- 暴露原始数据节点和值；
- 提供引用查找；
- 使用生成的 schema metadata 解释字段和类型；
- 为非法数据产物提供诊断。

生成代码职责：

- 声明目标语言类型；
- 声明字段、默认值、引用、enum、union 和顶层数据节点的 schema metadata；
- 将 schema type name 映射到目标语言构造函数；
- 为带 identity 的数据节点创建对象壳；
- 第二遍填充字段；
- 通过 identity map 解析引用；
- materialize 数组、字典、enum、nullable 和 union；
- 构建强类型配置数据库和可选索引。

加载算法需要两遍，以正确处理共享引用和循环引用：

```text
1. 读取 `.cfd` 数据文件或导出的图数据。
2. 通过生成的 schema metadata 解释数据。
3. 第一遍：为带 identity 的数据节点创建对象壳。
4. 将对象壳注册到 id -> object map。
5. 第二遍：填充对象字段。
6. 通过 id -> object map 解析引用。
7. 构建强类型顶层索引和查询 API。
```

生成的 C# 形态示例：

```csharp
public static GameConfigDatabase Load(Stream stream)
{
    var data = CfdReader.Read(stream);
    var context = new MaterializeContext(data, GameConfigSchema.Metadata);

    foreach (var node in data.Nodes)
    {
        context.Register(node.Id, CreateShell(node.Type));
    }

    foreach (var node in data.Nodes)
    {
        FillObject(context.Get(node.Id), node, context);
    }

    return GameConfigDatabase.Build(context);
}
```

生成的 loader 应加载 `.cfd` 或 `cfgraph`，永远不加载 `.cft`。

## 工具编辑边界

拆分后，工具直接编辑数据会更清晰：

- 工具默认编辑 `.cfd` 文件；
- `.cft` 文件作为 schema，通常由人维护；
- 外部编辑器可以修改 `.cfd`，不需要保留或理解类型定义；
- 生成数据可以放在专门的 `.cfd` 文件或 generated 目录中；
- 未来 source-editing API 可以基于数据路径工作，不会误改 schema。

为了支持稳健的工具编辑，后续应增加：

- lossless source document model，用于保留注释和格式；
- `cfc inspect --json`，输出 source span、数据路径和类型元数据；
- `cfc patch`，接收结构化源文件编辑；
- `cfc fmt`，提供稳定写回。

## 实施计划

1. 将文档中的语言概念从 CFC 调整为 `.cft` / `.cfd` 配置系统。
2. 根据文件扩展名增加 module kind 检测。
3. 拒绝 `.cfc` 作为输入扩展名。
4. 强制 `.cft` 和 `.cfd` 的语法/profile 规则。
5. 强制 import 方向规则。
6. 将示例和测试从 `.cfc` 改为 `.cft` / `.cfd`。
7. 更新 CLI help 和文档。
8. 更新 VS Code 扩展，识别 `.cft` 和 `.cfd`。
9. 增加 `cfc schema`。
10. 增加第一个目标语言生成器，优先考虑 C#。
11. 增加可选的 `cfc export`。
12. 为 `.cfs` 单独编写语言边界和 import 语义设计。
13. 为 Coflow Studio 单独编写产品和多视图编辑设计。
