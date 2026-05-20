# coflow核心配置

配置是顶层常量定义，由宿主加载，校验和消费。

## 配置定义

顶层`name = value`是配置定义。

```coflow
sword = {
  id: "sword",
  damage: 10,
}
```

顶层`name: Type = value`是带类型标注的配置定义。

```coflow
sword: Weapon = {
  id: "sword",
  damage: 10,
}
```

顶层`var`不是配置。

```coflow
var runtime_cache = null
```

## 常量表达式

配置值必须是常量，或者编译器可以确认为常量。

核心版本允许：

1. 字面量
2. 数组字面量
3. 对象字面量
4. 字典字面量
5. enum值
6. 函数值
7. `co fn`函数值
8. 简单算术
9. 字符串拼接
10. 同文件其他公开配置引用
11. import进来的其他配置引用

不允许：

1. 依赖顶层`var`
2. 调用普通函数
3. 调用宿主API
4. 访问IO，随机数，时间

函数也是值，因此函数值可以作为配置常量。配置常量求值阶段不执行函数体。

```coflow
skill = {
  id: "fireball",
  apply: fn(caster, target) {
    target.hp -= 10
  },
}
```

## 配置依赖

配置可以引用同文件中其他公开配置。

```coflow
base_damage = 10

sword = {
  id: "sword",
  damage: base_damage,
}
```

配置可以引用import模块中的公开配置。

```coflow
import weapons

starter = {
  id: "starter",
  damage: weapons.sword.damage,
}
```

配置依赖图必须无环。循环配置依赖是加载期错误。

```coflow
a = b + 1
b = a + 1
```

## 类型校验

带class类型标注的配置按class结构校验。

```coflow
class Weapon {
  id: string
  damage: int
}

sword: Weapon = {
  id: "sword",
  damage: 10,
}
```

校验规则：

1. 必填字段必须存在。
2. 字段类型必须匹配。
3. 有默认值字段可以省略。
4. 额外字段不允许。
5. 嵌套对象，数组和字典递归校验。
6. 如果class定义了`validate`块，在结构校验通过后执行。

未标注配置仍是合法配置，不做class闭合结构校验。

## 只读性

所有顶层配置对象都是深只读的。

```coflow
enemy = {
  hp: 100,
  items: ["coin"],
}
```

以下运行时修改都是错误：

```coflow
enemy.hp = 10
enemy.items[0] = "sword"
```

深只读包括嵌套对象，数组和字典。

## local与公开API

核心版本不支持私有配置定义。`local`用于私有`var`，`fn`，`co fn`，`class`和`enum`声明。

公开API不能泄露`local`类型。公开函数参数，返回值，公开class字段，公开顶层变量和公开配置类型都不能引用`local`类型。

## 配置错误

配置错误是加载期诊断，不通过脚本`try catch`捕获。

配置错误包括：

1. 非常量表达式。
2. 配置依赖循环。
3. 类型不匹配。
4. 缺失必填字段。
5. 带类型配置出现额外字段。
6. 修改只读配置。
7. `validate`校验失败。
