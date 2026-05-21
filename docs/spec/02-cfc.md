# cfc 配置语言

`.cfc` 是 coflow 的自校验强类型配置语言。它用于定义结构类型、枚举和值数据，并将数据保存为可加载、可校验、可保留引用关系的对象图。

`.cfc` 是纯数据语言，不执行运行时逻辑。`.cfc` 文件中不能定义函数、方法、运行时变量或控制流语句。

## 顶层结构

`.cfc` 文件的顶层由以下成员组成：

1. `use` 导入声明
2. `type` 类型定义
3. `enum` 枚举定义
4. 顶层数据定义

示例：

```coflow
use "common/types.cfc" as common;

type Stats {
  hp: int;
  speed: float;
}

enum Rarity {
  common;
  rare;
  epic;
}

local slime_stats: Stats = {
  hp: 30,
  speed: 1.0,
};

slime = {
  id: "slime",
  stats: slime_stats,
  rarity: Rarity.common,
};
```

## `use` 导入

`.cfc` 使用 `use` 导入其他 `.cfc` 文件：

```coflow
use "path/to/file.cfc" as name;
```

`use` 只加载配置文件中的定义和数据，不执行脚本代码。被导入文件的公开 `type`、`enum` 和数据定义通过别名访问：

```coflow
use "common/item.cfc" as item;

sword: item.Item = {
  id: "sword",
  rarity: item.Rarity.rare,
};
```

导入路径由宿主程序解析。`.cfc` 只能通过 `use` 引用其他 `.cfc` 文件，不能引用 `.cfs` 脚本文件。

## `type` 类型定义

`type` 用于定义数据结构。类型定义只能包含字段，不能包含方法。

```coflow
type Weapon {
  id: string;
  damage: int;
  cooldown: float = 1.0;
}
```

字段必须显式标注类型：

```coflow
type Weapon {
  id: string;       # 合法
  damage;           # 错误：字段必须标注类型
}
```

字段可以有默认值。默认值必须是 `.cfc` 数据值，不能调用函数或依赖运行时状态。

支持的字段类型包括：

- 基础类型：`int`、`float`、`bool`、`string`、`null`、`any`
- 数组类型：`[T]`
- 字典类型：`{K: V}`
- 当前文件或导入文件中的 `type`
- 当前文件或导入文件中的 `enum`

## `check` 数据校验

`check` 用于在 `type` 内定义数据校验规则：

```coflow
type Range {
  min: int;
  max: int;

  check {
    assert min <= max or "min must be <= max";
  }
}
```

`check` 块由若干 `assert` 语句组成：

```coflow
assert <bool-expr> or <string-expr>;
```

规则：

- `bool-expr` 为真时校验通过。
- `bool-expr` 为假时，求值 `string-expr` 作为校验错误信息。
- 多条 `assert` 按出现顺序求值，第一条失败即中止该对象的校验。
- `check` 中可以直接访问当前对象字段，不需要 `self.`。
- `check` 只允许纯数据表达式，不能调用宿主 API、修改状态或执行运行时逻辑。

`check` 是数据校验能力，不是脚本执行能力。

`check` 表达式需要复用 `.cfs` 的一部分表达式求值能力，例如比较、逻辑运算、字段访问和字符串表达式。为避免 `.cfc` v1 过早引入表达式执行模型，`check` 可以作为后续能力实现。`.cfc` v1 的必要校验能力先限定为结构校验、类型校验、默认值填充、必填字段和多余字段检查。

## `enum` 枚举定义

`enum` 定义有限的命名整数集合。语义与旧版 coflow 枚举一致。

```coflow
enum Rarity {
  common;
  rare;
  epic;
}
```

枚举变体默认从 `0` 开始自动编号，依次递增。可以显式指定整数值，未指定的变体从前一个值 +1 继续：

```coflow
enum Status {
  none = 0;
  active = 10;
  dead = 20;
  ghost;
}
```

使用枚举值通过 `EnumName.variant` 语法：

```coflow
rarity = Rarity.rare;
```

枚举底层表示为整数，但枚举类型与 `int` 不隐式互转。

## 数据定义

顶层数据定义用于声明命名数据节点：

```coflow
name = value;
name: Type = value;
local name = value;
local name: Type = value;
```

带 `local` 的数据节点只在当前 `.cfc` 文件中可见。未带 `local` 的数据节点可以被其他 `.cfc` 或 `.cfs` 通过对应命名空间引用。

顶层命名数据节点具有对象 identity。引用命名数据节点时，加载后的对象图保留共享引用关系：

```coflow
local shared_stats = {
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

## 解析器与数据模型职责

`.cfc` 实现不只是文本解析器，还需要提供可供宿主程序和 `.cfs` 使用的数据模型。它至少需要提供四类能力。

### 解析数据结构

解析器需要将 `.cfc` 文件解析为结构化表示，包括：

- `use` 导入声明
- `type` 类型定义
- `enum` 枚举定义
- 顶层数据定义
- 对象、数组、字典和基础字面量
- 当前文件和导入文件中的命名引用

解析阶段只建立语法结构和名称引用，不执行运行时逻辑。

### 构建可编辑对象图

解析器需要把 `.cfc` 数据构建为可访问的数据结构。该数据结构应能表达：

- 基础值
- 数组
- 对象
- 字典
- 枚举值
- 命名数据节点
- `local` 私有节点
- 内部引用和外部引用
- 对象 identity 和共享引用关系

宿主程序和 `.cfs` 脚本应能读取该对象图。若对象图以可变模式打开，使用方还应能修改字段、数组元素、字典条目和命名节点引用。

### 校验数据结构

校验阶段需要在解析结果基础上完成：

- 解析 `use` 依赖
- 收集并解析 `type` 和 `enum`
- 解析数据节点引用
- 构建对象图并保留共享引用
- 根据类型标注校验数据结构
- 填充字段默认值
- 检查必填字段缺失
- 检查多余字段
- 执行 `check` 数据校验（后续能力）
- 报告类型错误、引用错误和校验错误

校验成功后，`.cfc` 结果应是一份可供宿主程序或 `.cfs` 脚本使用的对象图。

### 保存数据结构

`.cfc` 数据结构需要支持被修改后再次保存为 `.cfc` 文本。保存能力由宿主程序决定是否开放；语言核心不默认授予脚本文件写权限。

保存时需要满足：

- 保存前重新校验数据结构。
- 保留命名节点和共享引用关系。
- 不可保存函数、脚本闭包、iterator、host object 等运行时值。
- 写入路径、权限、冲突处理、原子写入和格式保留策略由宿主程序负责。

`.cfc` 保存能力的目标是让宿主程序可以把修改后的对象图固化回配置文件，而不是让 `.cfc` 执行任何运行时逻辑。
