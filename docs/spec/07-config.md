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
2. 数组字面量（元素均为常量表达式）
3. 对象字面量（字段值均为常量表达式）
4. 字典字面量（条目值均为常量表达式）
5. 枚举值（`Rarity.common`）
6. 函数值（`fn` 或 `iter fn` 表达式本身，不是调用结果）
7. 简单算术：整数/浮点的 `+` `-` `*` `/` `//` `%` `**`
8. 字符串拼接（`+`）
9. 同模块其他公开配置的引用
10. 被 `import` 的模块的公开配置引用

**不允许：**

1. 引用顶层 `var`
2. 调用普通函数（函数值可以是配置，调用结果不是）
3. 调用宿主 API
4. 访问 IO、随机数、当前时间等运行时信息

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
```

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

**配置依赖图必须无环。** 循环依赖是加载期错误：

```coflow
a = b + 1;   # 错误：a 依赖 b，b 又依赖 a
b = a + 1;
```

编译器按拓扑序求值所有配置定义。

## 类型校验

### class 类型标注

带 class 类型标注的配置按 class 结构校验：

```coflow
class Weapon {
  id: string
  name: string
  damage: int
  cooldown: float = 1.0
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

check 块在结构校验通过后、配置值正式可用前执行。

check 块约束：
1. 只能读取 `self` 字段
2. 禁止调用宿主 API
3. 禁止修改任何状态
4. 可以使用纯计算（算术、比较、逻辑）

每条 check 语句 `condition => message`：条件为 `false` 时报告加载期错误，错误信息为 `message`。

```coflow
class Config {
  min: int
  max: int
  check {
    self.min <= self.max => "min must not exceed max"
  }
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
```

深只读覆盖：配置对象的直接字段、嵌套对象、嵌套数组及其元素、嵌套字典及其值。

配置中存储的函数值本身只读（不能被替换），但函数执行时可以修改函数自身捕获的外部可变状态。

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
