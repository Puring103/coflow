# 提案：切片语法

## 动机

Range 字面量已进入核心语言。数组切片是 Range 的自然延伸，在游戏开发中频繁出现：

```coflow
var tail   = queue[1..]          # 去掉队列头部
var recent = history[..10]       # 最近 10 条
var window = items[start..end]   # 滑动窗口
var copy   = items[..]           # 全复制
```

目前只能通过 `range` + 循环或宿主 API 模拟，代码冗长。

## 语法

切片是索引表达式 `expr[range]` 的特殊形式，当索引是 Range 时返回子数组。

```
slice_expr ::= expr "[" range_index "]"
range_index ::= expr? ".." "="? expr?
```

start 和 end 均可省略：

```coflow
items[2..5]    # [2, 5)，索引 2、3、4
items[2..=5]   # [2, 5]，索引 2、3、4、5
items[3..]     # 从索引 3 到末尾
items[..3]     # 从开头到索引 3（不含）
items[..]      # 全复制，等价于 items[0..len]
```

切片返回新数组（浅复制），不是视图（无引用语义）。

## 语义

- 越界行为：运行时报错，不裁剪。
- 负索引：核心版本不支持，放入后续提案。
- 字符串切片：核心版本不支持（字符串是 UTF-8，按字节切片语义不安全）。
- 切片结果类型与原数组相同。

## 语法冲突

Range 字面量 `0..10` 已有解析逻辑，切片复用同一套 token。

parser 需要区分：
- `items[0]`：整数索引
- `items[0..5]`：切片
- `items[x + y]`：表达式索引

区分点：索引表达式解析后，若遇到 `..` 或 `..=`，切换为切片路径。start 省略时，`[` 后直接是 `..`。

## 实现成本

低。

- AST：新增 `Expr::Slice { object, start: Option<Expr>, end: Option<Expr>, inclusive: bool }`
- Lexer：无变化，`..` 和 `..=` 已有。
- Parser：在索引表达式分支里，检测到 `..`/`..=` 后切换路径；start 省略的 `[..end]` 需要在 `[` 后前瞻一个 token。

## 开放问题

1. `items[..]` 是否等价于 `items` 的浅复制还是同一引用？
2. 字符串切片是否规划在类型系统成熟后引入？
