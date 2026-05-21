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

class Weapon { id: string }         # 公开 class

local class InternalState {         # 私有 class
  phase: int
}

base_damage = 10                    # 公开配置

local var _cache = null;             # 私有运行时变量
```

### 公开 API 约束

公开的声明不能泄露私有类型：

- 公开函数的参数类型或返回类型不能是 `local class` / `local enum`
- 公开 class 的字段类型不能是 `local` 类型
- 公开顶层 `var` 的类型标注不能是 `local` 类型
- 公开配置的类型标注不能是 `local` 类型

```coflow
local class Secret { value: int }

fn get_secret() -> Secret { ... }   # 错误：返回了私有类型

class Public {
  inner: Secret                     # 错误：字段引用了私有类型
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

但配置定义只能依赖拓扑序上已确定的其他配置，不允许循环依赖（见 [07-config.md](./07-config.md)）。

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
  hp: int

  fn take_damage(amount: int) {
    self.hp -= amount;
  }
}
```

`self` 只在 class 方法和 `check` 块内有效，其他位置使用 `self` 是错误。

## 模块加载阶段

模块加载分三个阶段，详见 [../design/05-runtime.md](../design/05-runtime.md)：

1. **声明收集**：扫描顶层，收集所有名称
2. **配置常量求值**：按依赖拓扑序求值所有配置，执行类型校验和 check 块
3. **运行时变量初始化**：初始化顶层 `var`，此后模块进入可调用状态

配置求值早于 `var` 初始化，因此配置不能依赖 `var` 的值。
