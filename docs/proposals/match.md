# P03 提案：match 表达式

## 动机

游戏脚本中经常按状态、枚举、字符串 id 或运行时类型分发逻辑。大量 `if / else if` 会让配置转换和状态机代码变得冗长。

`match` 提供表达式级分支，让“从一个值映射到另一个值”的代码更直接。

```coflow
var label = match rarity {
  Rarity.common => "Common",
  Rarity.rare => "Rare",
  Rarity.epic => "Epic",
  _ => "Unknown",
};
```

## 语法

第一期只做简单表达式匹配：

```coflow
match expr {
  pattern => expr,
  pattern => expr,
  _ => expr,
}
```

支持的 pattern：

```coflow
_                  # 默认分支
null
true
false
123
"text"
EnumName.variant
is Type
is not Type
```

示例：

```coflow
var damage_type = match element {
  "fire" => DamageType.fire,
  "ice" => DamageType.ice,
  _ => DamageType.normal,
};

var text = match value {
  is int => "integer",
  is string => value,
  null => "null",
  _ => to_string(value),
};
```

## 语义

- `match` 从上到下测试分支，第一条匹配成功的分支被选中。
- 每个分支右侧是表达式。
- `match` 本身是表达式，结果是被选中分支的右侧表达式结果。
- 若没有匹配分支，运行时错误；若静态可证明不穷尽，应提前诊断。
- 建议要求存在 `_` 默认分支，直到穷尽性分析成熟。

## 与联合类型和 `is` 的关系

`match` 可以配合 `is` 对联合类型做收窄：

```coflow
fn describe(value: int | string | null) -> string {
  return match value {
    is int => "int: " + value,
    is string => value,
    null => "missing",
  };
}
```

第一期类型收窄只要求支持简单变量名。

## 实现成本

中等偏高。

- Lexer：新增 `match` 关键字。
- AST：新增 `Expr::Match`、`MatchArm`、`Pattern`。
- Parser：表达式前缀位置解析 `match`。
- Sema：分支类型合并、`is` 分支收窄、默认分支检查。
- Runtime / config eval：按顺序测试分支并求值选中分支。

## 暂缓能力

第一期不做复杂模式匹配：

```coflow
{ id, damage } => ...
[head, ...tail] => ...
Some(x) => ...
```

这些能力应等解构和代数数据类型成熟后再统一设计。

## 开放问题

1. 第一版是否强制要求 `_` 分支。
2. enum 类型是否做穷尽性检查。
3. `match` 分支之间用 `,` 还是 `;` 分隔。
4. 分支右侧是否允许块体。
