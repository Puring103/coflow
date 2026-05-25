# cfc 配置语言

`.cfc` 是 coflow 的自校验强类型配置语言。它可以脱离 `.cfs` 独立运行，可被任意宿主语言加载，定位类似 JSON，但额外提供类型化枚举、对象 identity 和 schema 校验。

`.cfc` 是纯数据语言，不执行运行时逻辑。`.cfc` 文件中不能定义函数、方法、运行时变量或控制流语句。

注释使用 `//`：

```cfc
// 这是注释
name = "value";  // 行尾注释
```

## 顶层结构

`.cfc` 文件的顶层由三段组成，段之间不能交错：

1. `use` 导入声明
2. `type` 和 `enum` 定义（可混合，任意顺序）
3. 顶层数据定义

`type` 和 `enum` 支持所有前向引用，包括 `type` 引用 `enum`、`type` 引用 `type`、`enum` 引用无（枚举不引用其他定义）。数据定义之间也支持前向引用。

示例：

```cfc
use "common/types.cfc" as common;

type Stats {
  hp: int;
  speed: float;
}

enum Rarity {
  common,
  rare,
  epic,
}

type Monster {
  id: string;
  stats: Stats;
  rarity: Rarity;
  drop: common.Item;
}

slime_stats: Stats = {
  hp: 30,
  speed: 1.0,
};

slime: Monster = {
  id: "slime",
  stats: slime_stats,
  rarity: Rarity.common,
  drop: goblin_drop,     // 前向引用
};

goblin_drop: common.Item = {
  id: "coin",
  value: 5,
};
```

## `use` 导入

`.cfc` 使用 `use` 导入其他 `.cfc` 文件：

```cfc
use "path/to/file.cfc" as name;
```

`as name` 是必须的，不能省略。被导入文件的公开 `type`、`enum` 和数据定义通过别名访问：

```cfc
use "common/item.cfc" as item;

sword: item.Item = {
  id: "sword",
  rarity: item.Rarity.rare,
};
```

同一文件中重复 `use` 同一路径是错误，无论别名是否相同：

```cfc
use "common/item.cfc" as item;
use "common/item.cfc" as it;   // 错误：重复导入同一路径
```

重复导入判断基于宿主解析后的模块标识，而不是 `use` 中的原始路径字符串。若宿主把 `"./item.cfc"` 和 `"item.cfc"` 解析为同一个模块标识，则它们视为重复导入。

`use` 循环依赖完全允许：A `use` B、B `use` A 是合法的，loader 在构建阶段统一处理。

单文件 `.cfc`（无 `use`）完全自包含，是最小可用单元。`use` 是可选的多文件扩展能力。

导入路径由宿主程序解析。`imports()` 原样返回 `use` 声明中的路径字符串，宿主负责将其解析为模块标识，再通过 `bind_import()` 把这次解析结果交回 container；路径字符串和模块标识两者不要求格式一致。`.cfc` 只能通过 `use` 引用其他 `.cfc` 文件，不能引用 `.cfs` 脚本文件。

## `type` 类型定义

`type` 用于定义数据结构。类型定义只能包含字段，不能包含方法。字段之间用 `;` 分隔，允许末尾 `;`。

```cfc
type Weapon {
  id: string;
  damage: int;
  cooldown: float = 1.0;
}
```

字段规则：

- 字段必须显式标注类型
- 无默认值的字段必须填写
- 有默认值的字段可以省略；默认值必须是常量（字面量或枚举值，包括空数组 `[]`、空对象 `{}` 和空字典 `dict{}`），不能引用其他字段
- 默认值填充时会复制默认值本身；数组、对象和字典默认值不会在多个实例之间共享 identity

支持的字段类型：

- 基础类型：`int`、`float`、`bool`、`string`
- `any`：接受任意值，loader 只做引用合法性检查，不做类型匹配
- 数组类型：`[T]`，T 可以是任意合法字段类型，包括 `any`
- 字典类型：`{K: V}`，K 只允许 `string`、`int` 或任意已定义的 `enum` 类型名，V 可以是任意合法字段类型，包括 `any`
- 当前文件或导入文件中的 `type`
- 当前文件或导入文件中的 `enum`

`type` 使用名义类型（nominal typing），不使用结构类型。两个 `type` 即使字段完全相同，也不是同一个类型。对象字面量只有在上下文提供明确类型时才按该 `type` 校验；无类型标注的数据节点和 `any` 字段不会因为字段形状自动推断为某个 `type`。

```cfc
type A {
  id: string;
}

type B {
  id: string;
}

a: A = { id: "x" };
b: B = a;          // 错误：A 不是 B
raw = { id: "x" }; // 合法，但 raw 的结构不按 A 或 B 校验
```

`type` 支持前向引用和自引用：

```cfc
type Node {
  value: int;
  children: [Node];
}

type A {
  b: B;
}

type B {
  value: int;
}
```

### `check` 块（v2）

`check` 是 `type` 内部的可选数据校验块，v2 能力，v1 parser 遇到时直接跳过，不报错。`check` 块必须位于所有字段声明之后。

```cfc
type Range {
  min: int;
  max: int;

  check {
    min <= max;
    min >= 0;
  }
}
```

`check` 块由若干条件语句和 `all` 量词块组成，没有 `assert` 关键字，没有用户自定义错误消息。

**条件语句**：一个表达式加分号，求值结果必须为 bool。

```cfc
type Weapon {
  damage: int;
  cooldown: float;
  rarity: Rarity;

  check {
    damage > 0;
    cooldown >= 0.1;
    rarity >= Rarity.common;
    0 < damage <= 1000;
  }
}
```

**`all` 量词块**：对集合中每个元素执行内部条件，全部通过则通过。

```cfc
type Loot {
  drops: [Drop];

  check {
    all drop in drops {
      drop.value > 0;
      drop.rarity != Rarity.common;
    }
  }
}
```

`all` 支持 Array 和 Dict。迭代 Array 时绑定变量直接是元素；迭代 Dict 时绑定变量是 entry 对象，具有 `.key` 和 `.value` 字段：

```cfc
type ScoreTable {
  scores: {string: int};

  check {
    all entry in scores {
      entry.value >= 0;
    }
  }
}
```

`all` 块支持任意深度嵌套：

```cfc
type Zone {
  monsters: [Monster];

  check {
    all monster in monsters {
      monster.level > 0;
      all drop in monster.drops {
        drop.value > 0;
      }
    }
  }
}
```

**访问范围**：`type` 内 `check` 只能访问当前对象的字段和枚举值，不能引用外部命名节点。跨节点约束用顶层 `check` 表达。

**执行规则**：
- 多条语句顺序求值，条件为假时继续收集后续错误。
- 求值过程中发生类型错误或数组越界时立即停止当前对象的校验。
- 空集合上的 `all` 视为通过（vacuous truth）。
- 同一对象图中多处引用同一命名节点时，该对象的 check 只执行一次。

**支持的运算符**（优先级从低到高）：

- 逻辑：`||` `&&`，短路求值
- 比较：`==` `!=` `<` `<=` `>` `>=`，支持链式比较（方向一致，如 `0 < x <= 100`）
- 按位：`|` `^` `&`
- 算术：`+` `-` `<<` `>>` `*` `/` `//` `%` `**`，`**` 右结合
- 一元：`!` `~` `-`
- 后缀：`.`（字段访问）`[]`（索引访问）

枚举类型支持全部六种比较运算符，按底层整数值比较。

不支持：函数调用、字符串插值、变量声明、赋值、`?.`、`?[]`。

## `enum` 枚举定义

`enum` 定义有限的命名整数集合。变体之间用 `,` 分隔，允许末尾 `,`。

```cfc
enum Rarity {
  common,
  rare,
  epic,
}
```

枚举变体默认从 `0` 开始自动编号，依次递增。可以显式指定整数值，未指定的变体从前一个值 +1 继续。同一枚举内禁止重复整数值：

```cfc
enum Status {
  none = 0,
  active = 10,
  dead = 20,
  ghost,        // 自动为 21
}

enum Bad {
  a = 1,
  b = 1,        // 错误：重复值
}
```

使用枚举值通过 `EnumName.variant` 语法：

```cfc
rarity = Rarity.rare;
```

枚举底层表示为整数，但枚举类型与 `int` 不隐式互转。

同一文件内重复声明同名 `type`、`enum` 或数据节点是错误。`use` 别名不能与本地 `type`、`enum` 或数据节点同名：

```cfc
use "common/item.cfc" as item;

type item { ... }   // 错误：别名 item 与本地 type 同名
```

跨文件引用只允许一级别名访问，不能穿透多层 `use`：

```cfc
use "common/base.cfc" as base;

boss: base.Enemy = { ... };      // 合法：一级访问
x: base.other.Type = { ... };   // 错误：不允许多级穿透
```

## 字典字面量

字典使用 `dict{ }` 语法，与对象字面量 `{ }` 严格区分：

```cfc
dict{ "alice": 10, "bob": 20 }      // {string: int}
dict{ 1: "sword", 2: "shield" }     // {int: string}
dict{ DamageType.fire: 0.5 }        // {DamageType: float}
```

key 类型和 value 类型从字面量推断。空字典 `dict{}` 无法推断类型，必须带类型标注才合法：

```cfc
scores: {string: int} = dict{};    // 合法
scores = dict{};                    // 错误：无法推断类型
```

同一字典字面量中所有 key 必须类型一致，所有 value 必须类型一致。

## 数据定义

顶层数据定义用于声明命名数据节点：

```cfc
name = value;
name: Type = value;
```

所有顶层数据节点均公开，可以被其他 `.cfc` 或 `.cfs` 通过对应命名空间引用。`.cfc` 没有私有数据节点。

顶层命名数据节点具有对象 identity。引用命名数据节点时，加载后的对象图保留共享引用关系：

```cfc
shared_stats = {
  hp: 100,
};

slime = {
  stats: shared_stats,
};

goblin = {
  stats: shared_stats,
};
```

加载后，`slime.stats` 和 `goblin.stats` 指向同一个对象。

identity 规则：

- 顶层命名节点如果保存对象、数组或字典，则该值具有稳定 identity。
- 顶层命名节点如果保存基础值或枚举值，不提供可观察的对象 identity。
- 内联对象、数组和字典字面量不具有可被其他节点直接引用的独立 identity；它们是所属值的一部分。
- 只有通过命名节点引用同一个对象、数组或字典时，加载后才保留共享引用关系。

```cfc
shared_tags: [string] = ["enemy"];

slime = {
  tags: shared_tags,       // 与 goblin.tags 共享同一个数组
};

goblin = {
  tags: shared_tags,
};

orc = {
  tags: ["enemy"],         // 独立内联数组，不与 shared_tags 共享
};
```

数据值必须在加载期可完全解析为常量。合法的值形式包括：字面量（整数、浮点、布尔、字符串）、枚举值、对象字面量、数组字面量、字典字面量、以及对其他命名节点的引用（支持前向引用）。

数组字面量的元素必须类型一致，从元素推断数组类型。空数组 `[]` 无法推断类型，必须带类型标注才合法：

```cfc
[1, 2, 3]               // 合法，推断为 [int]
[1, "a"]                // 错误：元素类型不一致
items: [string] = [];   // 合法
items = [];             // 错误：无法推断类型
```

对象字面量的字段用 `,` 分隔，允许末尾 `,`。字典字面量 `dict{}` 内部条目同样用 `,` 分隔，允许末尾 `,`。

顶层数据节点声明以 `;` 结尾，必须显式写出：

```cfc
slime = { id: "slime" };    // 合法
slime = { id: "slime" }     // 错误：缺少末尾 ;
```

无类型标注的数据节点与 `any` 类型语义一致：不进行结构校验，但所有引用必须合法。

跨文件数据引用通过 `use` 别名访问：

```cfc
use "common/base.cfc" as base;

boss: base.Enemy = {
  id: "boss",
  hp: 500,
  drop: base.rare_loot,      // 引用导入文件中的数据节点
};
```

顶层命名节点支持循环引用，对象图允许有环。`build()` 遍历对象图时做 visited 标记以避免无限循环。循环引用只能通过命名节点表达：

```cfc
node_a = {
  value: 1,
  next: node_b,
};

node_b = {
  value: 2,
  next: node_a,
};
```

## 顶层 `check`（v2）

顶层 `check` 块用于约束跨节点的全局关系，是 `type` 内 `check` 做不到的能力。v2 能力，v1 parser 遇到时直接跳过，不报错。

顶层 `check` 属于数据定义段，必须位于所有 `type` 和 `enum` 定义之后，可以穿插在数据节点之间。一个文件可以有多个顶层 `check` 块。

```cfc
slime: Monster = {
  id: "slime",
  stats: slime_stats,
  rarity: Rarity.common,
};

goblin: Monster = {
  id: "goblin",
  stats: goblin_stats,
  rarity: Rarity.rare,
};

check {
  slime.stats.hp < goblin.stats.hp;
  goblin.rarity > slime.rarity;
}
```

语法与 `type` 内的 `check` 块完全一致，但访问范围是所在模块的顶层命名节点，而非 `self` 字段。`all` 量词块同样可用：

```cfc
monsters: [Monster] = [...];

check {
  all monster in monsters {
    monster.stats.hp > 0;
  }
}
```

**报错标识**：顶层 `check` 失败时以模块标识和行号定位，而非类型名。

v1 parser 必须能识别并跳过 `check { ... }` 块。跳过时需要正确处理字符串字面量、注释和嵌套括号，直到匹配到对应的右花括号。v1 不解析 `check` 内容，也不执行语义校验。

## Loader 接口

`.cfc` loader 本身无 I/O，路径解析和文件读取由宿主程序负责。所有加载操作通过 `CfcContainer` 完成。

### 低层接口

```
container.add_module(name, source)
```

注册并解析一个 `.cfc` 源文本。`name` 为宿主解析后的模块标识，不要求等同于文件路径。container 保存源码和解析后的 AST；同一 `name` 重复注册是错误。

```
container.replace_module(name, source)
```

原子替换一个已注册模块的源码和 AST。`name` 必须已存在。替换前先解析新源码；若解析失败，container 保持原状态。替换成功后清空该模块发出的所有 import 绑定，其他模块指向该模块的绑定保留。

```
container.imports(name) -> [ImportDecl]
```

返回指定模块的 `use` 声明列表。每个 `ImportDecl` 至少包含稳定的 import id、别名、原始路径字符串和源码位置。宿主使用原始路径字符串完成路径解析，但绑定时使用 import id，而不是使用 path 字符串作为 key。

```
container.bind_import(name, importId, dependencyName)
```

注册模块 `name` 中某条 `use` 声明对应的依赖模块标识。`dependencyName` 必须已经通过 `add_module()` 或 `replace_module()` 成功注册。同一个 import id 不能重复绑定。同一模块内不允许多条 `use` 绑定到同一个 `dependencyName`，也不允许多个 `use` 使用同一个别名。

```
container.build(rootName) -> CfcResult
```

从 `rootName` 出发，对 root 的 import closure 执行结构校验和对象图构建。返回结果包含 root closure 中的所有模块。宿主既可以直接取 root 模块导出，也可以按模块名取任意 closure 模块导出。

```
container.build_all() -> CfcResult
```

对 container 中所有已注册模块执行结构校验和对象图构建。该接口主要用于工具、编辑器和多 root 场景；普通加载入口应优先使用 `build(rootName)`。

`build()` 内置结构校验：必填字段检查、类型匹配、默认值填充、多余字段检查。结构不合法时报告错误，不返回对象图。

`build()` 的处理顺序：

1. 确定要构建的模块集合：`build(rootName)` 使用 root import closure，`build_all()` 使用所有已注册模块。
2. 建立每个模块的 `use`、`type`、`enum` 和顶层数据节点符号表。
3. 校验同名定义、重复导入、未绑定 import、未知引用、跨模块别名访问和不允许的多级穿透。
4. 为构建集合内所有顶层命名对象、数组和字典创建占位节点，用于支持前向引用和循环引用。
5. 解析并连接所有数据引用边，包括跨文件引用。
6. 在上下文类型存在的位置执行结构校验，包括必填字段、多余字段、字段类型、数组元素类型、字典 key/value 类型和枚举类型。
7. 对省略字段填充默认值；对象、数组和字典默认值按值复制，不与其他实例共享 identity。
8. 返回对象图结果。若任一步出现结构错误，`build()` 报告错误且不返回对象图。

### 高层接口

```
container.load_graph(rootName, rootSource, resolver)
```

一键加载 root import closure 并构建结果。`rootName` 不能已存在。`resolver` 是宿主提供的回调函数，签名为 `(fromName, importDecl) -> (dependencyName, source)`。`dependencyName` 是宿主解析后的模块标识，`source` 是该模块源码。

`load_graph()` 内部执行：

1. 调用 `add_module(rootName, rootSource)` 注册 root。
2. 读取每个模块的 `imports()`。
3. 对每条 import 调用 resolver，获得 `(dependencyName, source)`。
4. 若 `dependencyName` 尚未注册，则调用 `add_module(dependencyName, source)`；若已经注册，则忽略本次返回的 source。
5. 调用 `bind_import(fromName, importId, dependencyName)`。
6. 所有可达依赖加载完后调用 `build(rootName)`。

resolver 可能会因为多条 import 解析到同一个 `dependencyName` 而被多次调用；如源码读取成本较高，宿主应在 resolver 内部缓存。

大多数场景使用 `load_graph()`；需要精细控制依赖加载顺序、缓存、热重载或路径重映射时使用低层接口。

### Rust reference API

Rust crate 是 CFC 的第一版参考实现。spec API 描述稳定概念模型；Rust API 可以使用强类型、错误集合和 ownership 规则表达这些概念。

模块标识使用强类型，不直接混用原始路径字符串：

```rust
pub struct ModuleId(String);
pub struct ImportId(u32);
```

`ModuleId` 由宿主 resolver 产生，loader 不负责路径规范化。原始 `use` 路径只保存在 `CfcImport.path` 中，用于宿主解析和诊断。

核心类型：

```rust
pub struct CfcImport {
    pub id: ImportId,
    pub alias: String,
    pub path: String,
    pub span: Span,
}

pub struct CfcContainer { ... }

pub struct CfcResult { ... }

pub struct CfcModuleResult { ... }
```

低层 API：

```rust
impl CfcContainer {
    pub fn new() -> Self;

    pub fn add_module(
        &mut self,
        module: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), ParseErrors>;

    pub fn replace_module(
        &mut self,
        module: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), ParseErrors>;

    pub fn imports(&self, module: &ModuleId) -> Result<&[CfcImport], ModuleError>;

    pub fn bind_import(
        &mut self,
        from: &ModuleId,
        import: ImportId,
        dependency: &ModuleId,
    ) -> Result<(), BindImportError>;

    pub fn build(&self, root: &ModuleId) -> Result<CfcResult, BuildErrors>;

    pub fn build_all(&self) -> Result<CfcResult, BuildErrors>;

    pub fn check(&self, result: &CfcResult) -> Vec<CheckError>;
}
```

`add_module()` 和 `replace_module()` 会立即完成词法和语法解析，并保存源码和 AST。`add_module()` 遇到重复 `ModuleId` 报错。`replace_module()` 遇到不存在的 `ModuleId` 报错；替换是原子的，解析失败时 container 不变。

`imports()` 返回借用 slice。调用方若需要在随后调用 `bind_import()`，应先复制出 `ImportId` 和 `path`，避免同时持有不可变借用和可变借用。

高层 API：

```rust
impl CfcContainer {
    pub fn load_graph<R>(
        &mut self,
        root: ModuleId,
        source: impl Into<String>,
        resolver: R,
    ) -> Result<CfcResult, CfcError>
    where
        R: FnMut(&ModuleId, &CfcImport) -> Result<(ModuleId, String), ResolveError>;
}
```

`load_graph()` 每个 `ModuleId` 最多解析一次。若 resolver 多次返回已经注册的 `ModuleId`，本次返回的 source 被忽略；source 一致性由宿主负责，未来可以增加 strict/debug 检查。

错误类型：

```rust
pub struct ParseErrors {
    pub errors: Vec<ParseError>,
}

pub struct BuildErrors {
    pub errors: Vec<BuildError>,
}

pub enum CfcError {
    Parse(ParseErrors),
    Module(ModuleError),
    Import(BindImportError),
    Resolve(ResolveError),
    Build(BuildErrors),
}
```

低层 API 使用分阶段错误类型；`load_graph()` 使用统一 `CfcError`。parser 第一版可以 fail-fast，但 API 使用 `ParseErrors` 预留错误恢复能力。`build()` 应尽量收集多个结构错误后一次性返回 `BuildErrors`。

结果访问：

```rust
impl CfcResult {
    pub fn root_id(&self) -> Option<&ModuleId>;
    pub fn root(&self) -> Option<&CfcModuleResult>;
    pub fn module(&self, module: &ModuleId) -> Option<&CfcModuleResult>;
    pub fn modules(&self) -> impl Iterator<Item = (&ModuleId, &CfcModuleResult)>;
}

impl CfcModuleResult {
    pub fn get(&self, name: &str) -> Option<CfcValueRef>;
    pub fn values(&self) -> impl Iterator<Item = (&str, CfcValueRef)>;
}
```

`build(root)` 返回的 `CfcResult` 同时暴露 root 模块和 root closure 内所有模块。`build_all()` 返回的 `CfcResult` 没有唯一 root，`root_id()` 和 `root()` 返回 `None`。

### check 校验接口（v2）

```
container.check(result) -> [CheckError]
```

对 `build()` 返回的对象图执行 `check` 块校验。返回错误列表而非抛异常，方便编辑器等工具场景一次性展示全部校验错误。

`check()` 只在 `build()` 成功后执行。`check()` 不修改对象图，不填充默认值，不重新执行结构校验。`type` 内 `check` 以每个已构建对象为上下文执行；顶层 `check` 以所在模块的顶层命名节点为上下文执行。

## CLI

CFC 提供命令行工具，是 `CfcContainer` 接口的薄封装，resolver 直接读取文件系统。

```
cfc check <file>    # 加载并校验，报告全部错误
cfc fmt <file>      # 格式化
```

## 错误阶段

CFC 错误按阶段划分：

- `add_module` / `replace_module` 阶段：词法错误、语法错误、段落顺序错误、缺少分隔符、非法字面量、重复模块或缺失模块。
- `imports` 阶段：指定模块尚未注册。
- `bind_import` 阶段：指定模块尚未注册、import id 不存在、依赖模块尚未注册、重复绑定、同一模块内多个 import 绑定到同一依赖模块、别名重复。
- `build` 阶段：重复定义、未绑定 import、未知符号、非法跨模块访问、类型不匹配、缺少必填字段、多余字段、无法推断空数组或空字典类型、枚举重复值、非法循环类型约束。
- `check` 阶段：条件表达式为假、`check` 表达式类型错误、`check` 中访问不存在的字段或越界数组下标。

`add_module`、`replace_module`、`imports`、`bind_import` 和 `build` 的错误会阻止返回可用对象图。`check` 错误不改变对象图，返回错误列表供宿主或编辑器展示。

## v1 / v2 边界

| 能力 | v1 | v2 |
|------|:--:|:--:|
| 结构校验（必填字段、类型匹配、默认值填充、多余字段） | ✓ | |
| `use` 多文件导入 | ✓ | |
| `CfcContainer` 低层接口 | ✓ | |
| `CfcContainer.load_graph` 高层接口 | ✓ | |
| CLI `check` / `fmt` | ✓ | |
| `type` 内 `check` 块语义校验 | | ✓ |
| 顶层 `check` 块 | | ✓ |
| `CfcContainer.check()` | | ✓ |
