# CFD 数据

> 范围：记录、结构化字段值、引用和函数实现。

## 1. 文件与记录

```text
cfd-file := { use-decl } { record-decl }
record-decl := record-key ":" qualified-name "{" [ field-list ] "}"
record-key := identifier
field-list := field { "," field } [ "," ]
field := identifier ":" value
```

CFD 可以导入名称，顶层记录的类型必须为 table 或单例，data 只用于字段等数据位置。
CFD 不声明 namespace、类型、字段默认值或 check。
@Host 服务由宿主整对象提供，CFD 不声明该服务的记录或覆盖其实现。
record key 是非保留标识符，在所属整棵继承树内唯一；父子类型和兄弟类型不能使用相同 key。
无共同父类型的独立类型允许同 key。类型名使用完整限定名或显式导入的短名。

```cfd
use game::Item;

sword: Item {
  name: "Sword",
  price: 100,
}
```

字段顺序不改变类型规则；来源顺序和源码位置保持稳定。
加载时完成类型检查、默认值补齐、函数编译和引用链接。check 由调用方单独选择执行。

## 2. 字段值

值文法见[类型与值](./03-类型与值.md)。
普通数据使用数字、布尔、字符串、enum、集合、对象、记录引用和 None。
函数值与 fstring 可以出现在各自类型允许的位置。

```cfd
item: Item {
  name: "Sword",
  label: f"名称：{self.name}",
  price: 100,
  tags: ["weapon", "fire"],
  next: None,
  total: fn(count: int) -> int {
    self.price * count
  },
}
```

- 省略字段时使用默认值；无默认值的可选字段为 None，其余字段必填。
- 显式 None 表示可选字段为空。
- 字段和字典 key 不能重复。
- fstring 使用 `f"..."` 或可访问的 fstring 常量初始化，不能写普通字符串替代。
- 普通数据位置不执行任意表达式；函数体和 fstring 插值使用完整表达式。

## 3. 集合与对象

```cfd
tags: ["weapon", "rare"],
weights: { "fire": 10, "ice": 5 },
stats: Stats { hp: 100, speed: 1.5 },
effect: DamageEffect { label: "Hit", amount: 20 },
```

数组保留元素顺序，字典保留键值项在原始 CFD 中的声明顺序。
使用 CFT 默认值或常量字典时保留其原始 CFT 声明顺序，加载器不重新排序。
`{ ... }` 是字典；内联对象必须写 `DataName { ... }`，类型名必须指向 data。
table 和单例通过记录引用使用，不用对象字面量内联。
抽象类型位置必须提供具体子类型。

集合中的函数不受额外位置限制。普通读取 fstring 元素时计算文本，获取集合本身不计算元素。

## 4. enum 与 flag

enum 使用变体名或限定名。flag 支持 `&`、`^`、`|` 和括号，
优先级为 `&` 高于 `^` 高于 `|`，与函数表达式一致。

```cfd
permissions: Permission::Read | Permission::Write,
```

操作数必须属于同一个 flag 类型。

## 5. 记录引用

```text
reference-value := "&" [ qualified-name "::" ] record-key
```

- `&key` 按源码所在的静态本类型查找记录；引用其他类型时写 `&Type::key`。
- 不按目标字段类型猜测其他记录类型，也不进行全项目短 key 搜索。
- 没有本类型上下文的位置使用带类型写法。
- 显式类型采用普通名称解析规则。
- 被引用类型必须是 table 或 singleton；data 没有可查询的记录目录。
- 目标记录必须存在，实际类型必须可赋给引用字段的目标类型。
- 引用可以跨文件、自引用或形成跨记录循环。
- 引用不是字符串，不能加引号。

```cfd
sword: Item {
  name: "Sword",
  next: &shield,
  owner: &Character::hero,
}
```

记录引用在普通函数和插值表达式中也使用相同写法。
指定类型的引用查找覆盖该类型及其子类型；key 唯一性检查覆盖整棵继承树。
例如字段声明写 `next: Item?`，CFD 提供的引用值写 `next: &Item::shield`。
静态上下文和继承示例见[命名空间](./08-命名空间.md#5-引用与-record-key)，插值使用相同规则。

## 6. fstring 与函数

```cfd
item: Item {
  name: "Sword",
  label: f"名称：{self.name}",
  total: fn(count: int) -> int {
    self.price * count
  },
  price: 100,
}
```

普通字符串中的花括号只是文本。
fstring 的插值支持参数、局部变量、常量、只读 self、记录引用及函数调用。
源码保存原始模板；读取时得到本次计算的 string。

CFD 函数必须写完整签名并为所有参数命名，与 CFT 声明匹配。
它只替代对应对象的函数实现，不修改契约代码。

CFD 字段及字段集合中直接声明的函数和 fstring 绑定字段所属对象。
嵌套对象自身字段中的字面量绑定嵌套对象；维度基础值和覆盖值中的绑定见[维度](./09-维度.md)。

## 7. 维度数据

维度数据直接写在所属业务记录的字段中，使用 `dimension { default: ..., variant: ... }`。
缺失或未知变体回退基础值；可选字段的显式 `None` 是有效覆盖。
具体结构见[维度](./09-维度.md)。
