# coflow class与配置校验设计

class是coflow的主要结构声明方式
用于定义对象结构，方法集合和配置校验规则

配置是顶层常量定义
配置必须在编译期确定

# class基础语法
class声明字段结构

```coflow
class Weapon {
  id: string
  damage: int
  cooldown: float
}
```

# 字段默认值
字段可以声明默认值

```coflow
class Enemy {
  id: string
  hp: int = 100
  speed: float = 1.0
}
```

有默认值的字段可以在配置中省略

```coflow
slime: Enemy = {
  id: "slime"
}
```

# 配置定义
顶层`name = value`定义自动视为配置
顶层`name: Type = value`表示带类型标注的配置

```coflow
sword = {
  id: "sword"
  damage: 10
}

typed_sword: Weapon = {
  id: "sword"
  damage: 10
  cooldown: 0.8
}
```

# 编译期常量
配置必须在编译期确定
配置值必须是常量，或者编译器可以确认为常量

允许：
1. 字面量
2. 数组，字典，对象字面量
3. enum值
4. 函数值
5. co fn函数值
6. 简单算术
7. 字符串拼接
8. 同文件其他配置项引用
9. import进来的其他文件配置常量

不允许：
1. 依赖顶层var的运行时结果
2. 调用普通函数
3. 调用宿主API
4. 访问IO，随机数，时间

# 结构校验
带类型标注的配置按class结构校验

校验内容：
1. 必填字段必须存在
2. 字段类型必须匹配
3. 有默认值字段可以省略
4. 可为null字段必须显式使用联合类型
5. 嵌套class实例递归校验
6. 数组，字典元素递归校验

```coflow
class Drop {
  item_id: string
  weight: int
}

drop: Drop = {
  item_id: "coin"
  weight: 100
}
```

# 缺失字段
没有默认值的字段缺失时报错

```coflow
bad_drop: Drop = {
  item_id: "coin"
}
```

# 额外字段
带类型标注的配置不允许额外字段

```coflow
bad_drop: Drop = {
  item_id: "coin"
  weight: 100
  color: "gold"
}
```

未标注类型的配置允许任意字段

```coflow
raw_drop = {
  item_id: "coin"
  weight: 100
  color: "gold"
}
```

# 自定义校验
class可以定义特殊校验函数validate

```coflow
class Range {
  min: int
  max: int

  fn validate(self) -> null | string | [string] {
    if self.min > self.max {
      return "min must be <= max"
    }

    return null
  }
}
```

validate返回值规则：
1. null表示通过
2. string表示单条错误
3. [string]表示多条错误
4. 其他返回值表示校验函数错误

# 校验顺序
配置校验顺序：
1. 编译期常量检查
2. 结构校验
3. 嵌套字段递归校验
4. validate自定义校验

# 方法语法
class可以定义方法
方法使用显式self参数

```coflow
class Player {
  hp: int

  fn damage(self, amount: int) {
    self.hp -= amount
  }
}
```

# 对象可变性
普通对象可变
配置项是常量绑定
配置对象默认只读

```coflow
var player: Player = {
  hp: 100
}

player.hp -= 10
```

配置对象不允许运行时修改

```coflow
enemy: Player = {
  hp: 100
}

enemy.hp -= 10 // 不允许
```

# 暂不支持
1. class继承
2. 多继承
3. trait
4. interface
5. 复杂构造函数
6. 隐式self
