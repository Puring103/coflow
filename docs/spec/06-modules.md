# coflow 模块与作用域

## 模块

一个文件是一个模块。模块的文件路径决定其导入路径。

模块顶层只允许声明（`import`、`class`、`enum`、`fn`、`iter fn`、`var`、配置定义），不允许普通运行时语句。

## 可见性

顶层声明默认**公开**，可以被其他模块通过 `import` 访问。

`local` 关键字声明**私有**成员，只在当前文件内可见：

```coflow
fn public_fn(x) { return x * 2; }   # 公开

local fn private_fn(x) { return x + 1; }  # 私有

class Weapon { id: string; }        # 公开 class

local class InternalState {         # 私有 class
  phase: int;
}

base_damage = 10;                   # 公开配置

local var _cache = null;             # 私有运行时变量
```

### 公开 API 约束

公开的声明不能泄露私有类型：

- 公开函数的参数类型或返回类型不能是 `local class` / `local enum`
- 公开 class 的字段类型不能是 `local` 类型
- 公开顶层 `var` 的类型标注不能是 `local` 类型
- 公开配置的类型标注不能是 `local` 类型

```coflow
local class Secret { value: int; }

fn get_secret() -> Secret { ... }   # 错误：返回了私有类型

class Public {
  inner: Secret;                    # 错误：字段引用了私有类型
}
```

## 作用域层级

coflow 的作用域从内到外：

1. **块作用域**：`{ }` 内声明的 `var` 和局部 `fn`
2. **函数作用域**：函数参数
3. **模块作用域**：顶层的所有声明
4. **导入作用域**：被 `import` 导入的模块

名称查找从最内层作用域向外逐层查找，找到第一个匹配项为止。

### 块级作用域

`var` 声明的变量从声明处到所在块末尾可见：

```coflow
fn example() {
  var x = 1;

  {
    var y = 2;
    print(x);   # 可见
    print(y);   # 可见
  }

  print(x);     # 可见
  print(y);     # 错误：y 已超出作用域
}
```

内层块可以**遮蔽**（shadow）外层同名变量：

```coflow
var x = 1;
{
  var x = 2;    # 遮蔽外层 x
  print(x);     # 2
}
print(x);       # 1
```

### 模块作用域与前向引用

顶层所有声明在整个文件内互相可见，不受声明顺序限制：

```coflow
fn a() {
  b();    # 合法，b 在模块作用域内
}

fn b() {
  print("b");
}
```

但配置定义的值依赖必须无环（见 [07-config.md](./07-config.md)）。

## 循环导入

模块之间允许声明级循环导入。编译器先扫描所有参与加载的模块并收集顶层声明，因此以下关系可以成立：

- 类型之间互相引用
- 函数签名之间互相引用
- 函数体互相调用
- class、enum、fn 等声明跨模块成环

循环导入本身不是错误。错误只来自需要立即求值的值依赖成环，例如配置值、字段默认值或顶层 `var` 初始化互相依赖。

```coflow
# a.cf
import b;

fn use_b() {
  b.helper();
}

# b.cf
import a;

fn helper() {
  a.use_b();
}
```

上例是合法的声明级循环。函数体在运行期调用，模块加载时不需要立即求出对方函数的返回值。

## 闭包与捕获

函数可以捕获定义处外层作用域的局部变量，形成闭包。

**捕获语义：共享引用。** 闭包持有对变量本身的引用，而非值的拷贝。闭包内外对同一变量的修改互相可见：

```coflow
fn make_counter() {
  var count = 0;
  return fn() {
    count += 1;
    return count;
  };
}

var c = make_counter();
c();   # 1
c();   # 2
```

多个闭包共享同一变量：

```coflow
fn make_pair() {
  var value = 0;
  var inc = fn() { value += 1; };
  var get = fn() => value;
  return { inc: inc, get: get };
}

var p = make_pair();
p.inc();
p.inc();
p.get();   # 2
```

### 可捕获的变量

函数只能捕获**外层局部变量**（`var` 声明的变量和函数参数）。顶层声明（`fn`、配置、`var` 等）通过模块作用域直接访问，不经过闭包捕获机制。

### 循环变量捕获

`for in` 的循环变量在每次迭代中是同一个绑定，多个闭包捕获同一循环变量时共享同一引用：

```coflow
var fns = [];
for i in 0..3 {
  fns += [fn() => i];
}
# 循环结束后，所有闭包中 i 的值相同（循环已结束时的状态）
```

## self

`self` 是 class 方法和 `check` 块内的隐式参数，指向当前对象实例：

```coflow
class Player {
  hp: int;

  fn take_damage(amount: int) {
    self.hp -= amount;
  };
}
```

`self` 只在 class 方法和 `check` 块内有效，其他位置使用 `self` 是错误。

## 模块加载阶段

模块加载分四个阶段，详见 [../design/05-runtime.md](../design/05-runtime.md)：

1. **声明收集**：扫描所有相关模块的顶层声明，收集所有名称
2. **类型与签名解析**：解析 class、enum、函数签名和公开 API 约束
3. **配置常量求值**：按配置依赖拓扑序求值所有配置，执行类型校验和 check 块
4. **运行时变量初始化**：初始化顶层 `var`，此后模块进入可调用状态

配置求值早于 `var` 初始化，因此配置不能依赖 `var` 的值。

## import 路径与可见性

### 路径到文件的映射

`import a.b.c` 的查找规则：

1. 以 `.` 分隔的段映射为目录层级，最后一段是文件名（不含扩展名）
2. 文件扩展名固定为 `.cf`
3. 查找根由宿主程序提供（一般是工程根目录）
4. `import a.b.c` 对应 `<root>/a/b/c.cf`

包级别的 "目录即模块" 形态在核心版本不支持（不存在 `mod.cf` / `init.cf` 之类的入口约定）。

### 非传递可见性

`import` 只引入被显式导入模块的公开成员，**不传递**：若 `a` 导入了 `b`，`c` 导入了 `a`，`c` 中不能直接访问 `b` 的成员；要使用 `b`，`c` 必须自己 `import b`。

### `as` 别名

`import path as name` 的别名仅在当前文件内生效，不影响其他模块对该 path 的访问名。

## 预定义全局名

coflow 在所有模块中预定义两个全局命名空间，它们不是关键字，但作为预定义名不可被声明、赋值或遮蔽：

| 名称 | 作用域 | 说明 |
|------|--------|------|
| `env` | 加载期 + 运行期，全局只读 | 加载期注入的常量命名空间，详见 [07-config.md](./07-config.md) |
| `host` | 仅运行期 | 宿主程序提供的运行期 API 入口；加载期访问 `host` 是错误（配置常量表达式不允许） |

## 内建函数

以下名称在所有模块中可见，行为由语言核心定义（不属于 `host`）：

| 名称 | 签名（近似） | 说明 |
|------|--------------|------|
| `print` | `fn(any...)` | 打印调试信息到宿主默认输出，参数按 `to_string` 规则转换 |
| `error` | `fn(message: string, data: any = null) -> Error` | 创建 Error 对象，详见 [04-statements.md](./04-statements.md) |
| `to_string` | `fn(any) -> string` | 显式转换为字符串，规则同 `+` 拼接 |

数组、字典、字符串等内置类型的方法（`push`、`len` 等）不属于全局函数，通过点访问调用，详见 [02-types.md](./02-types.md)。

## 协程与并发的非目标

coflow 核心版本**不**提供以下能力，使用方应改由宿主程序处理：

- 双向协程（`send` / `throw` 注入、跨函数 yield）
- async / await
- 多线程并发
- 抢占式 / 协作式调度器

`iter fn` 是单向 yield 的简单生成器，仅服务于迭代场景。
