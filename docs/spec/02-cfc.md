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

`use` 循环依赖完全允许：A `use` B、B `use` A 是合法的，loader 在构建阶段统一处理。

单文件 `.cfc`（无 `use`）完全自包含，是最小可用单元。`use` 是可选的多文件扩展能力。

导入路径由宿主程序解析。`getusing()` 原样返回 `use` 声明中的路径字符串，宿主负责将其解析为调用 `parse()` 时使用的模块标识，两者不要求格式一致。`.cfc` 只能通过 `use` 引用其他 `.cfc` 文件，不能引用 `.cfs` 脚本文件。

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

支持的字段类型：

- 基础类型：`int`、`float`、`bool`、`string`
- `any`：接受任意值，loader 只做引用合法性检查，不做类型匹配
- 数组类型：`[T]`，T 可以是任意合法字段类型，包括 `any`
- 字典类型：`{K: V}`，K 只允许 `string`、`int` 或任意已定义的 `enum` 类型名，V 可以是任意合法字段类型，包括 `any`
- 当前文件或导入文件中的 `type`
- 当前文件或导入文件中的 `enum`

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
    assert min <= max : "min must be <= max";
  }
}
```

`check` 块由若干 `assert` 语句组成：

```cfc
assert <bool-expr> : <string-expr>;
```

- `bool-expr` 为真时校验通过；为假时求值 `string-expr` 作为错误信息。
- 多条 `assert` 按出现顺序求值，第一条失败即中止该对象的校验。
- `check` 中直接访问当前对象字段，不需要 `self.`。
- 只允许纯数据表达式，不能调用宿主 API、修改状态或执行运行时逻辑。
- 表达式语义在 CFC 规范自身内定义，不依赖 CFS。

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
  assert slime.stats.hp < goblin.stats.hp : "goblin should be stronger than slime";
}
```

语法与 `type` 内的 `check` 块完全一致，但访问的是顶层命名节点而非 `self` 字段。

## Loader 接口

`.cfc` loader 本身无 I/O，路径解析和文件读取由宿主程序负责。所有加载操作通过 `CfcContainer` 完成。

### 低层接口

```
container.parse(name, source)
```

注册一个 `.cfc` 源文本。`name` 为该模块的标识（通常是文件路径）。同一 `name` 重复注册是错误。

```
container.getusing(name) -> [path]
```

返回指定模块通过 `use` 声明的、尚未注册到 container 的依赖路径列表。宿主可循环调用直到返回空列表，完成依赖发现。

```
container.build() -> CfcResult
```

对所有已注册模块执行全量解析和结构校验，返回全部模块的导出数据。宿主按模块名从结果中取用。

`build()` 内置结构校验：必填字段检查、类型匹配、默认值填充、多余字段检查。结构不合法时报告错误，不返回对象图。

### 高层接口

```
container.import(name, source, resolver)
```

一键导入。`resolver` 是宿主提供的回调函数，签名为 `path -> source`。`import` 内部自动驱动依赖发现循环，调用 `resolver` 获取依赖文件内容，全部就位后自动执行 `build()`。

大多数场景使用 `import`；需要精细控制依赖加载顺序、缓存或路径重映射时使用低层接口。

### check 校验接口（v2）

```
container.check(result) -> [CheckError]
```

对 `build()` 返回的对象图执行 `check` 块校验。返回错误列表而非抛异常，方便编辑器等工具场景一次性展示全部校验错误。

## CLI

CFC 提供命令行工具，是 `CfcContainer` 接口的薄封装，resolver 直接读取文件系统。

```
cfc check <file>    # 加载并校验，报告全部错误
cfc fmt <file>      # 格式化
```

## v1 / v2 边界

| 能力 | v1 | v2 |
|------|:--:|:--:|
| 结构校验（必填字段、类型匹配、默认值填充、多余字段） | ✓ | |
| `use` 多文件导入 | ✓ | |
| `CfcContainer` 低层接口 | ✓ | |
| `CfcContainer.import` 高层接口 | ✓ | |
| CLI `check` / `fmt` | ✓ | |
| `type` 内 `check` 块语义校验 | | ✓ |
| 顶层 `check` 块 | | ✓ |
| `CfcContainer.check()` | | ✓ |
