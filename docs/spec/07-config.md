# coflow 配置系统

配置是 coflow 的核心能力。配置定义在顶层，由宿主程序统一加载、校验和消费，与运行时脚本共享同一语法。

## 配置定义

顶层 `name = value` 是无类型配置定义，`name: Type = value` 是带类型标注的配置定义，均以 `;` 结尾：

```coflow
base_damage = 10;

sword: Weapon = {
  id: "sword",
  damage: 10,
};
```

语法：

```
name = const_expr
name: TypeExpr = const_expr
```

与顶层 `var` 的区别：

|            | 配置定义                  | 顶层 var              |
|------------|---------------------------|-----------------------|
| 语法       | `name = v` / `name: T = v` | `var name = v`        |
| 只读性     | 深只读                    | 可修改                |
| 求值时机   | 加载期（配置常量求值阶段） | 运行时（模块初始化）  |
| 值约束     | 必须是常量表达式          | 任意表达式            |

## 常量表达式

配置值必须是**常量表达式**——加载期可完全求值的表达式。

**允许：**

1. 字面量（整数、浮点、布尔、字符串、`null`）
2. 插值字符串 `f"..."`，要求所有内嵌 `{ expr }` 也是常量表达式
3. 数组字面量（元素均为常量表达式）
4. 对象字面量（字段值均为常量表达式）
5. 字典字面量（条目值均为常量表达式）
6. 枚举值（`Rarity.common`）
7. 函数值（`fn` 或 `iter fn` 表达式本身，不是调用结果）
8. 简单算术：整数/浮点的 `+` `-` `*` `/` `//` `%` `**`
9. 字符串拼接（`+`）和 `string + 任意常量值` 的隐式 `to_string`
10. 比较运算（`==` `!=` `<` `<=` `>` `>=`），含链式比较
11. 逻辑运算（`and` `or` `not`）和空值合并 `??`
12. `in` / `not in`，右侧为 Range 字面量或常量数组/字典
13. **`if` 表达式**（条件、两个分支体的最后值都必须是常量表达式；要求两个分支都存在）
14. 同模块其他公开配置的引用
15. 被 `import` 的模块的公开配置引用
16. **`env.field` 访问**（字段必须在宿主提供的 schema 中声明，详见下文"env 命名空间"）

**不允许：**

1. 引用顶层 `var`
2. 调用普通函数（函数值可以是配置，调用结果不是）
3. 访问 `host` 或调用宿主 API
4. 访问 IO、随机数、当前时间等运行时信息
5. 循环结构（`while` / `for in`）
6. `try` / `catch` / `throw`
7. 赋值或副作用（包括对 `env` 字段的写入）

```coflow
base_damage = 10;
bonus = base_damage * 2;    # 合法：引用配置 + 简单算术

var scale = host.get_scale();
damage = scale * 10;        # 错误：scale 是 var，不是常量

skill = {
  id: "fireball",
  apply: fn(caster, target) {   # 合法：函数值本身是常量
    target.hp -= 10;
  },
};

# if 表达式 + env：随平台变化的常量
quality = if env.platform == "mobile" { "low"; } else { "high"; };
log_level = if env.debug { 0; } else { 3; };
```

`if` 表达式作为常量表达式时，**两个分支体都必须是常量表达式**（不是只有被选中那条），目的是让依赖图分析可以在不求值的情况下完成。

## env 命名空间

`env` 是宿主程序在加载期注入的全局只读命名空间，用于让配置参数化外部输入（平台、构建模式、版本号、地区等）。

### 注入与 schema

宿主程序在执行 coflow 加载流程之前，提供：

1. 一份 **env schema**：声明所允许字段及其类型，例如
   ```
   { platform: string, debug: bool, region: string, build_id: int }
   ```
2. 一份与 schema 匹配的字段值表

加载期访问 `env.field`：

- 字段在 schema 中存在 → 视为该类型的常量值
- 字段不在 schema 中存在 → **加载期错误**（不会返回 `null` 或抛运行时错误）

### 求值时机

`env` 在阶段 1（声明收集）之前就已经填充完毕，因此：

- `env.x` 在配置常量表达式中视为常量
- 配置依赖图中 `env.field` 是叶子节点，不参与依赖排序
- 引用 `env.field` 的配置随宿主每次加载时的 `env` 取值而变化（不缓存跨次加载）

### 与 `host` 的区别

| 命名空间 | 阶段 | 可变性 | 用途 |
|----------|------|--------|------|
| `env` | 加载期可读，运行期可读 | 加载前注入，模块加载期间和运行期均不变 | 平台、debug 标志、版本号等"环境"参数 |
| `host` | 仅运行期 | 宿主可变 | 调用游戏 API（`host.spawn(...)` 等） |

配置常量表达式只能用 `env`，不能用 `host`；运行期代码两者均可读。

### 不可遮蔽

`env` 和 `host` 都是预定义全局名，不能被声明、赋值或遮蔽。试图 `var env = ...;` 或 `import x as env;` 是错误。

## 配置依赖

配置可以引用同模块的其他公开配置，也可以引用 `import` 进来的模块的公开配置：

```coflow
base_damage = 10;

sword = {
  id: "sword",
  damage: base_damage,         # 引用同模块配置
};
```

```coflow
import weapons;

starter_pack = {
  weapon: weapons.sword,
  bonus: weapons.base_damage,  # 引用导入模块的配置
};
```

配置依赖图必须无环。依赖图优先使用字段级粒度，而不是简单地把整个配置对象当作一个节点。

例如：

```coflow
a = b.x + 1;

b = {
  x: 10,
  y: a + 1,
};
```

这是合法的。真实依赖是：

```text
a   -> b.x
b.y -> a
```

图中没有环。

下面的写法是循环依赖：

```coflow
a = b.y + 1;

b = {
  x: 10,
  y: a + 1,
};
```

真实依赖是：

```text
a   -> b.y
b.y -> a
```

图中存在环，因此是加载期错误。

字段级依赖的目标是减少误报，但不是所有表达式都必须无限细分。第一版可以采用以下保守归并规则：

- 直接字段访问 `config.field` 记录到字段级节点
- 直接引用整个配置 `config` 记录到配置级节点
- 数组元素依赖归并到整个数组值
- 字典 key 访问归并到整个字典值
- 对象展开 `...base` 依赖整个 `base`
- `check` 块依赖整个被校验对象
- 函数值本身是常量；配置求值阶段不展开函数体依赖

编译器按依赖图拓扑序求值配置值和字段。若保守归并后的图出现环，仍按循环依赖报告。

## 类型校验

### class 类型标注

带 class 类型标注的配置按 class 结构校验：

```coflow
class Weapon {
  id: string;
  name: string;
  damage: int;
  cooldown: float = 1.0;
}

sword: Weapon = {
  id: "sword",
  name: "Iron Sword",
  damage: 15,
  # cooldown 有默认值，可以省略
};
```

校验规则（按顺序）：

1. **必填字段必须存在**：无默认值的字段不能省略
2. **字段类型必须匹配**：每个字段的值类型必须与 class 声明一致
3. **有默认值的字段可以省略**：省略时使用 class 的默认值
4. **不允许多余字段**：出现 class 未声明的字段是错误
5. **递归校验**：嵌套对象、数组、字典中的元素按对应类型递归校验
6. **check 块**：结构校验全部通过后执行 class 的 `check` 块（如果有）

### 无类型标注的配置

未标注类型的配置合法，但不进行 class 闭合结构校验：

```coflow
settings = {
  volume: 80,
  fullscreen: false,
};
```

## check 块执行

check 块在结构校验通过后、配置值正式可用前执行。块内由若干 `assert <bool-expr> or <string-expr>;` 语句组成，详细语法见 [02-types.md](./02-types.md) 的"check 块"。

check 块约束：

1. 仅允许读取 `self` 字段、`env`、引用其他配置；可调用纯方法
2. 禁止调用宿主 API（`host`）
3. 禁止修改任何状态
4. 可以使用纯计算（算术、比较、逻辑、`if` 表达式、字符串拼接 / 插值）

`bool-expr` 求值为假时，求值 `string-expr` 作为加载期错误信息；多条 `assert` 按出现顺序求值，第一条失败即中止该实例的校验。

```coflow
class Config {
  min: int;
  max: int;

  check {
    assert self.min <= self.max or "min must not exceed max";
  };
}

level_range: Config = { min: 1, max: 100 };
# 若 min > max，加载期报错
```

## 只读性

所有顶层配置对象加载完成后为**深只读**，包括所有嵌套层级：

```coflow
enemy = {
  hp: 100,
  items: ["coin", "key"],
};
```

以下运行时修改均为错误：

```coflow
enemy.hp = 50;            # 错误
enemy.items[0] = "sword"; # 错误
enemy.items.push("orb");  # 错误：调用会原地修改的内建方法
```

深只读覆盖：配置对象的直接字段、嵌套对象、嵌套数组及其元素、嵌套字典及其值。

### class 方法与深只读

class 实例作为配置时同样深只读。**对深只读对象进行任何写入都会触发运行时错误**，包括通过会写入 `self.field` 的方法间接写入：

```coflow
class Vector {
  x: float;
  y: float;

  fn scale(factor: float) {
    self.x *= factor;       # 写 self.x
    self.y *= factor;
  };
}

origin: Vector = { x: 0.0, y: 0.0 };

origin.x = 1.0;             # 运行时错误：writing read-only field 'x'
origin.scale(2.0);          # 运行时错误：方法体执行 self.x *= 时触发
```

判定规则：写入是否合法**取决于运行时目标对象的只读标记**，与方法本身的声明形态无关。同一 class 实例若不是配置（运行时新建）则可写：

```coflow
fn make() -> Vector {
  return Vector{ x: 1.0, y: 1.0 };   # 运行时新建，非只读
}

var v = make();
v.scale(2.0);               # 合法
```

不进行编译期检查；规则简单一句话："写操作命中只读对象时运行时报错"。

### 函数值与捕获状态

配置中存储的函数值本身只读（不能被替换），但函数执行时可以修改**该函数自身捕获的外部可变状态**——闭包捕获的可变变量不属于配置只读范围：

```coflow
local var counter = 0;

bump = fn() {
  counter += 1;             # 修改捕获的运行时变量，合法
};
```

## 配置错误

配置错误是加载期诊断，中止模块加载，不通过脚本 `try catch` 捕获。

错误类型：

1. 非常量表达式（依赖了运行时才能确定的值）
2. 配置依赖循环
3. 类型不匹配（字段值类型与 class 声明不符）
4. 缺失必填字段
5. 多余字段（在带类型标注的配置中）
6. 运行时尝试修改只读配置对象
7. check 块校验失败
