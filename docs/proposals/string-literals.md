# 提案：字符串字面量扩展

核心版本已经支持普通字符串，原始字符串，多行字符串和原始多行字符串。

## 候选能力

字符串插值：

```coflow
var text = "hello ${name}"
```

原始字符串：

```coflow
var path = r"C:\game\assets\hero.png"
```

多行字符串：

```coflow
var text = """
line one
line two
"""
```

原始多行字符串：

```coflow
var shader = r"""
float4 main() {
}
"""
```

## 待解决问题

1. 插值表达式的求值时机。
2. 插值是否允许任意表达式。
3. 插值是否自动调用字符串转换。
4. 是否支持格式化选项。
