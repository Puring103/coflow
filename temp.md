我把 7 份 spec 又通读了一遍。下面是严苛的审查，按严重性分层。

## 一、关键洞 — 必须补

### 1. 算术语义大量未定义（最严重）

`02-types.md` 把 `int`/`float`/`bool` 列了，但运算语义几乎没说：

- `int / int` 的结果类型 — 是 `int`（截断，Rust 风格）还是 `float`（Python 3 风格）?spec 里**完全没写**。这是写第一个 `5 / 2` 表达式的人就要知道的事。

- `int + float` 是否自动 promote 到 `float`?没说。

- 整数溢出:`9_000_000_000_000_000_000 * 2` 是 panic、wrap、还是 saturate?没说。配置/嵌入式场景应该 panic(运行时错误)更安全。

- 除以 0:`1/0`、`1.0/0.0`、`0.0/0.0` 分别返回什么?没说。

- `%` 取余的负数语义:`-7 % 3` 是 `2`(Python)还是 `-1`(C)?没说。

- `` 的结果类型**:`2 -1` 是 float?`2 64` 溢出怎么办?没说。

- 位运算只支持 `int` 吗?`1.5 & 1` 怎么办?没说。

- `<<` 的负移位 / 大移位:`1 << -1`、`1 << 100` 怎么办?没说。

这一组洞影响的是日常表达式,优先级最高。

### 2. `var x: int;` 的默认值矛盾

- `04-statements.md` L39:`var cache;` 默认为 `null`。

- `02-types.md` L51:`var hp: int = null` 是错误。

合起来:**`var hp: int;` 既要默认 `null`,又禁止 `int` 持 `null`** —— 直接矛盾。

建议二选一:(a) 类型标注的 var 必须显式初始化;(b) 类型标注的 var 用类型默认值(`int=0`、`string=""`、`bool=false`、`[T]=[]`、`{K:V}=dict{}`)。我倾向 (a),更显式。

### 3. `assert` 的"块表达式"与 spec 自相矛盾

- `02-types.md` L348:`bool-expr` 可以是任意纯表达式,**包括块表达式**。

- `03-expressions.md` L239:**coflow 不提供"块作为表达式"的语法形态**。

二者直接打架。要么删掉 02 的"块表达式",改写为"如果需要中间变量,使用立即调用匿名函数 `(fn() {...})()`";要么 03 增加一节"块表达式只在 if 分支和 assert 中可用"。我建议前者(对实现更友好)。

### 4. `==` / `<` 跨类型语义未定义

- `[1] == [1]` 是 `true`(深相等)还是 `false`(引用相等)?

- `"a" < "b"` 合法吗?字典序?

- `null 0` / `null false` / `null == null` ?

- `Rarity.epic > Rarity.rare` 合法吗(同 enum ordering)?

- `1 == 1.0` 是 true 吗?

这些都是用户立刻会问的问题,spec 应有一节"相等性 / 排序"规则表。

### 5. 集合类型的引用 vs 值语义

`02-types.md` 描述了对象/数组/字典的字面量,但**没定义赋值语义**:

```coflow

var a = [1, 2, [3]];

var b = a;

b[2].push(4); # a 看得见吗?

```

是引用语义(共享)还是值拷贝?Lua 是引用,Python 也是引用。建议明示"对象/数组/字典是引用语义,赋值不拷贝;字符串和基础类型是值语义"。

### 6. 顶层 `var` 初始化顺序未定义

```coflow

var a = b + 1;

var b = 10;

```

`06-modules.md` L93 说"顶层声明在整个文件内互相可见,不受声明顺序限制"——这对函数/class 是对的,但 var 初始化是有运行时顺序的。spec 没说初始化按什么顺序、引用未初始化的 var 怎么办。

建议:顶层 `var` 初始化按文件声明顺序(从上到下);引用尚未初始化的同模块 `var` 是加载期错误(可静态检查)或视为 `null`(动态)——选其中之一并写明。

## 二、解析层歧义 / 不一致

### 7. 表达式体函数的分号规则不一致

- `01-overview.md` L39:"以 `}` 结尾的构造**不需要**分号,包括 `fn`、`iter fn` ..."

- `05-declarations.md` L86 的例子:

 ```coflow

 fn double(x: int) -> int => x * 2

 ```

 顶层声明,**不以 `}` 结尾**,但例子里没有分号。

按 01 的规则字面解读,这个例子是错的。需要补一条:"表达式体的 `fn` / `iter fn` 声明,表达式后必须 `;` 终结"——或者反过来明示"`=>` 引入的表达式体也归为不需要分号的构造"。我倾向前者(更一致)。

### 8. `Ident{` 的"紧贴"在跨行时含义不清

```coflow

var v = Vector

{ x: 1, y: 2 };

```

`03-expressions.md` 说"标识符紧贴 `{`,无空白或操作符隔开"——空白是否包含换行?如果包含,这种写法被拒,对长字段对象不友好。如果不包含,那就有歧义(可能解析成 `Vector`(表达式语句) 后面跟一个新的对象字面量?但没分号,无法分隔)。

建议明示:"紧贴"指标识符与 `{` 之间不含 token 分隔(可有空格但不可有换行/注释/操作符),否则 `Ident{...}` 不再触发类型化对象字面量解析。

### 9. f-string 的 `{}` 内能不能含 `:`?

```coflow

f"{x:.2f}" # Python 风格的格式控制?

f"{ obj: int }" # 类型标注?显然不行,但 ":" 出现了

f"{ {a: 1}.a }" # 内嵌对象字面量?

```

`03-expressions.md` 没明示 `:` 的处理。最务实:**核心版禁止 `{}` 内裸 `:`**(嵌套对象/字典字面量除外,因为它们的 `{...}` 自包含),并明示不支持 Python 风格的 `:.2f` 格式控制。需要格式时显式调用 `to_string` 或宿主提供的格式化函数。

### 10. f-string 嵌套的层数

`03-expressions.md` L143 允许 `f"{ f\"...\" }"`。建议核心版直接禁止嵌套 f-string——实现复杂度高,人工读起来也极差。

## 三、对象/方法 / 协议的细节

### 11. 方法引用的 bound/unbound 语义

```coflow

class Foo { fn bar() { ... } }

var f: Foo = ...;

var m = f.bar; # m 是什么?bound 方法?未绑定函数?

m(); # self 是 f 还是错?

```

spec 没明示。Python 是 bound,JS 不是。建议明示:**方法引用是 bound**,即 `f.bar` 等价于 `fn(...args) => f.bar(...args)`。否则 `var m = f.bar; m()` 是个常见 footgun。

### 12. 方法内 `self.field` 是否可省略

```coflow

class Foo {

 x: int;

 fn m() {

 print(x); # 错误(x 未声明)?还是隐式 self.x?

 }

}

```

spec 没说。Rust/Python 强制 `self.`,JS 不要。建议:**强制显式 `self.field`**(没有隐式 `this`),与 Rust/Python 一致,可读性更好。

### 13. `Iterator` 协议的"约定 vs 协议"

`02-types.md` L457 用了 `iter` 和 `next` 两个魔法名字:

- 用户 class 的 `fn iter()` 必须返回 Iterator

- Iterator 对象必须有 `next` 字段且是函数

但这两个名字不是关键字,完全可以被 class 用作其他用途——`for in` 时类型不匹配只能运行时崩。这在动态语言里可以接受,但 spec 应明示**"如果定义 `iter` 方法,签名/语义必须符合 Iterator 协议;否则 `for in` 行为未定义/运行时报错"**。

未来引入 interface 时再形式化。

### 14. `self` 不可重新绑定

```coflow

fn m() {

 self = other; # 错?

}

```

按 spec 没明示,但显然不该允许。补一句"`self` 不可重新绑定,只能修改 `self.field`"。

## 四、配置 / 加载期 边角

### 15. 常量 `if` 表达式分支体的限制

`07-config.md` 允许 `if` 作为常量表达式,但分支是块,块允许多条语句:

```coflow

config = if env.debug {

 var x = compute(); # 函数调用,非常量

 x;

} else { 0; };

```

spec 没明示分支体内能不能有 `var` 声明、循环等。建议补一句:**"在常量上下文中,`if` 分支体内只允许常量表达式语句(包括 `var` 但其初值必须是常量,不允许循环 / try 等)"**。最严格的写法:**分支体退化为单条 `expr;`**。

### 16. 配置中的 `fn` 字面量捕获什么

```coflow

sword = {

 apply: fn(c, t) {

 counter += 1; # 引用顶层 var

 host.foo(); # 引用 host

 }

};

```

加载期没有"调用栈",fn 字面量不创建闭包(没有局部变量可捕获)。它就是一个"匿名顶层函数"。运行时调用时通过模块作用域引用 var/host。

spec 应明示这一点,否则用户会以为存在某种"加载期闭包"。

### 17. `env` schema 的来源

`07-config.md` 说宿主提供 schema,但**没说在哪里提供** —— 是在 .cf 源码里写,还是宿主 API 注入?读完整个 spec 还是不知道。建议补一句:"env schema 由宿主程序在加载流程外提供(API 调用,不在 .cf 中声明)"。

### 18. 重复 import / import 名冲突

```coflow

import a.b.c;

import a.b.c; # 重复?

import a.b.c as foo;

import x.b.c; # 同名 c 冲突?

```

spec 06 说"`import a.b.c` 引入名字 `c`"(隐式)——但这条**实际没明确写**。然后冲突如何处理也没说。建议:

- 同一文件重复 import 同一路径(不论别名)是错误。

- import 的访问名是 `as` 别名;不写 `as` 时是路径最后一段。

- 同一文件中两个不同路径产生同名访问名是错误,必须用 `as` 区分。

### 19. `throw` 非 Error 对象的检测

`04-statements.md` L287:"`throw` 的操作数必须是 Error 对象"——这是动态语言,所以是运行时检查。但 spec 应明示这是**运行时类型错误**(throw operand must be Error),否则实现可能会"throw 任意值,catch 拿到任意值",破坏 Error 的字段不变量。

### 20. rethrow 的 stack 行为

```coflow

try { throw error("a"); }

catch e { throw e; } # stack 保留还是更新?

```

spec 没说。建议:**rethrow 保留原 stack**(不覆盖)——便于追溯首次抛出位置。

## 五、内置类型方法的缺口

`02-types.md` 只列了 `.len` 和 `arr.push(x)`,但实际写脚本立刻就会需要:

- string:`.contains`、`.starts_with`、`.ends_with`、`.split`、`.replace`、`.trim`、`.upper/lower`、`.index_of`

- array:`.pop`、`.remove(i)`、`.clear`、`.contains`、`.index_of`、`.sort`、`.reverse`、`.slice(a, b)`

- dict:`.keys()`、`.values()`、`.remove(k)`、`.contains(k)`(也可用 `in`)

- 数值:`abs`、`min`、`max`、`floor`、`ceil`、`round` —— 这些是函数还是方法?

spec 应该明示:**核心提供哪些 / 哪些丢给 stdlib**。否则同一段 coflow 代码在不同实现下不可移植。

## 六、长期可扩展性

### 21. 关键字预留不足

未来很可能加的功能:模式匹配、类型别名、可见性标注、可变性标注、interface/trait。建议**现在就保留**:`match`、`case`、`type`、`pub`、`mut`、`const`、`interface`、`trait`、`impl`、`let`。否则将来加这些功能会破坏兼容。

`from` 现在只为 `yield from` 保留,可以考虑改用 `yield* expr`(JS 风格)释放 `from`,但这是次要决定。

### 22. 命名规范缺失

spec 01 没有命名规范一节(class PascalCase、fn/var snake_case、enum 变体 snake_case、配置 snake_case、常量?)。建议加一节,明示惯例。

### 23. 错误层级没有系统化

spec 散落了"词法错误"、"加载期错误"、"运行时错误"等术语,但没有集中定义有几种错误层级、各自能否被 catch、有什么诊断格式。建议在 spec 01 或新建 `08-errors.md` 集中定义:

- 词法 / 语法错误(parse 时)

- 解析 / 类型 / 循环依赖错误(加载期阶段 1-2)

- 配置错误(加载期阶段 3,含 check 失败)

- 初始化错误(加载期阶段 4)

- 运行时错误(可被 `try catch` 捕获)

- 内部 panic(实现 bug,不可被脚本捕获)

每类的诊断格式、是否中止 VM 等也要明示。

### 24. 跨阶段可见性矩阵

`07-config.md` 和 `06-modules.md` 分别说了 env/host/var/配置在不同阶段的可读写情况,但没集中成表。建议加一张表:

| | 加载期阶段 1-2 | 加载期阶段 3 (配置求值) | 加载期阶段 4 (var 初始化) | 运行期 |

| ---------- | -------------- | ----------------------- | ------------------------- | ------ |

| `env` | 可读 | 可读 | 可读 | 可读 |

| `host` | 错误 | 错误 | 可读写 | 可读写 |

| 配置 | 待求值 | 按拓扑序求值 | 只读 | 只读 |

| 顶层 `var` | 待初始化 | 错误 | 按声明顺序初始化 | 可读写 |

## 七、性能/footgun 提示

### 25. `arr += [x]` 的 O(n²) 警告

spec 钉了"非原地"语义,但**没在 spec 提示性能陷阱**。在循环里 `arr += [x]` 是 O(n²),与 `arr.push(x)` 性能差别巨大。建议在 02-types.md 数组节加一行:

> 注意:循环内追加优先使用 `arr.push(x)`;`arr += [x]` 每次都创建新数组,O(n²)。

### 26. Range 是不是值

```coflow

var r = 0..10; # 合法?

host.foo(0..10); # 合法?

```

spec 03 说 Range "只能用于 `for in` 或 `in` 判断" —— 但这是限制 Range 的"作用域"(只在这些上下文出现),还是说 Range 不是一等值?如果不是一等值,语法上需要明示禁止,parser 也得做特殊处理。

建议:Range 是一等值(可放变量、可传参),只是当前没有 length / map / 其他方法。这更简单一致。

---

## 我会优先修这些

如果只能修一部分,我建议按以下优先级:

1. 算术语义(`int/int`、promotion、溢出、除零、负数 `%`) — 最高优,影响每个表达式

2. `var hp: int;` 默认值矛盾 — 一行就能说清,但现在自相矛盾

3. `==` / `<` 跨类型语义、引用 vs 值语义 — 用户每天都问的问题

4. assert 中"块表达式"的矛盾 — 内部不一致,必须二选一

5. 常量上下文中 if 分支体的限制 — 否则常量表达式定义有漏洞

6. 方法引用 bound / `self.x` 不可省略 — 决定一类常见 bug 模式

7. 关键字预留 — 现在不预留,将来加功能就要破坏兼容

其余(命名规范、内置方法清单、错误层级、可见性矩阵、性能提示)是质量层面的补充,可以批量处理。

需要我针对其中任何一条草拟具体的 spec 修订?
