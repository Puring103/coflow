# 引用、继承和多态

## 独立记录与内联对象

共享、可独立增删、可按 ID 查找的对象使用 table 与 `&Table`。只属于父对象的一段结构使用 data，字段直接写 data 类型。

```cft
table Item { name: string; }
abstract data Reward { source: string = "drop"; }
sealed data ItemReward: Reward { item: &Item; count: int = 1; }
sealed data CurrencyReward: Reward { amount: int; }
table Quest { rewards: [Reward] = []; }
```

```cfd
sword: Item { name: "铁剑" }
first: Quest {
  rewards: [
    ItemReward { item: &Item::sword, count: 2 },
    CurrencyReward { amount: 100 },
  ],
}
```

Reward 是抽象字段类型，必须提供具体子类型。内联 data 不需要全局 ID；引用目标必须存在，可跨文件、成环，文件要一起加载。

## 继承边界

- table 继承 table，data 继承 data，singleton 可继承 table；singleton 本身封闭。
- abstract 类型不能直接实例化；sealed 类型不能被继承。
- 字段在继承链中不得重名，不通过重复声明覆盖父字段或默认值。
- 子类型可赋给父类型位置，反向不可；引用可以指向目标 table 的具体派生记录。
- singleton 不能直接用作字段类型或引用目标；其唯一记录通过 singleton 入口访问。
- `@struct` 适用于没有基类的 sealed data，不用于多态继承叶子。

table 继承树共享 ID 唯一域；两个无关根类型可以有同名 ID。对具体记录修改时使用查询返回的实际类型；查询父类是否包含子类由命令 include-derived 选项明确控制。

## 规则和迁移

共享对象约束放在 data 自身，所有嵌套实例都会检查。父规则先于子规则执行，命名规则在继承链内唯一。check 中 `value is ConcreteType` 判断实际类型，成立后可访问子类字段。

改变继承关系时同时核对：同树 ID 冲突、继承字段、引用赋值、内联实际类型、规则名称和覆盖目标。改类型名称不会自动重写任意源文件；用完整文件候选一次更新相关 CFT 与 CFD，见[结构变更](schema-migration.md)。
