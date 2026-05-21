# P04 提案：局部 const 常量

## 动机

coflow 目前只有 `var`，所有局部绑定都默认可重新赋值。配置系统强调只读，但运行时代码缺少轻量表达“这个绑定不会被重新绑定”的方式。

`const` 用于局部不可重新绑定变量，提升可读性，也为未来静态诊断和优化提供基础。

```coflow
fn damage(player, weapon) {
  const base = weapon.damage;
  const crit = player.crit_rate > 0.5;
  return if crit { base * 2; } else { base; };
}
```

## 语法

```coflow
const name = expr;
const name: Type = expr;
```

`const` 必须初始化：

```coflow
const x;        # 错误
const hp: int;  # 错误
```

## 语义

- `const` 绑定不可重新赋值。
- `const` 的不可变性只作用于“绑定本身”，不自动深冻结引用值。
- 如果 `const arr = []`，不能写 `arr = other`，但是否允许 `arr.push(x)` 取决于数组值本身是否只读。
- `const` 遵守普通局部作用域和 shadow 规则。
- 顶层配置定义已经是深只读，不需要写 `const`。

```coflow
const arr = [];
arr = [1];        # 错误：不能重新绑定 const
arr.push(1);      # 合法，除非 arr 指向只读配置对象
```

## 顶层 const

第一期不引入顶层 `const`。顶层已有两类声明：

- 配置定义：加载期常量，深只读。
- `var`：运行时模块变量，可修改。

若增加顶层 `const`，需要定义它和配置定义的关系，容易造成重复概念。

## 与配置常量表达式的关系

局部 `const` 不等于配置常量。它只是运行期不可重新绑定的局部变量：

```coflow
fn f() {
  const n = host.random();   # 合法，运行期常量绑定
}
```

## 实现成本

低。

- Lexer：新增 `const` 关键字。
- AST：局部声明增加 mutability，或新增 `ConstDecl`。
- Parser：语句位置解析 `const`。
- Sema：赋值目标检查时禁止写入 const 绑定。
- HIR：局部变量记录 `mutable: bool`。

## 开放问题

1. `const` 是否允许用于函数参数。
2. 是否需要 `var` / `const` 的 formatter 规则。
3. 未来是否引入顶层 `const`，还是坚持顶层配置定义承担该角色。
