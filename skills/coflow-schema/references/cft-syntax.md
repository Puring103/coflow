# 编写 CFT 声明

适用于需要新增或修改声明时。所有选中的 CFT 文件共享全局名称，无 import；同名声明冲突。文件为 UTF-8，`#` 开始行注释，字段以分号结束。

## 基本声明

```cft
enum Quality { Common, Rare = 10, Epic }
const MAX_PRICE: int = 10000;
type ItemLink = Option<&Item>;

sealed data Stats { hp: int = 100; }
table Item {
  @localized
  name: string;
  price: int;
  quality: Quality = Quality::Common;
  stats: Stats = Stats {};
  next: ItemLink = None;
  check ValidPrice { price >= 0 && price <= MAX_PRICE; }
}
singleton Settings { start: &Item; }
```

CFD 必须提供 Settings 同名记录及 Item 必填值。规则语法另见[check 规则](check-rules.md)。

## 类型

| 语法 | 约束 |
| --- | --- |
| int / long | 有符号 32 / 64 位整数 |
| float / double | 有限 binary32 / binary64；float 按 32 位精度舍入 |
| bool / string | 布尔、普通或显式模板字符串 |
| `Option<T>` | 保留每层 Some/None |
| `[T]` | 有序数组 |
| `{K: V}` | K 为 int、long、string 或 enum，键唯一 |
| data 名称 | 内联对象 |
| `&TableName` | table 记录引用 |

类型别名 `type Name = T;` 与常量可跨文件使用，依赖不能成环。常量可推断无歧义类型，None 和空集合需要明确类型；引用需要记录上下文，不能作为普通全局常量值。

数值表达式按 int → long → float → double 提升，已声明类型不能隐式缩窄。无目标类型的整数按范围推断 int/long，浮点默认 double；有目标类型的字面量按目标检查。

## 枚举与注解

```cft
@flag enum Permission { Read = 1, Write = 2, Execute = 8 }
@int64 enum LargeCode { Special = 4294967296 }
enum ItemId {}
@idAsEnum(ItemId)
table NamedItem { name: string; }
```

普通 enum 默认 int，省略数值时递增，成员名称及值不能重复。@int64 使用 long。@flag 自动提供 None=0 和 Everything（实际标志位的按位或），不能自行声明这两个名称；非零成员必须是正的二次幂。@flag 与 @int64 可组合。

@idAsEnum 要引用已声明的空 enum，用于 table ID 映射；新增记录仍写 CFD，不手工向该 enum 填记录成员。

| 注解 | 用途 |
| --- | --- |
| `@label("文本")` / `@description("文本")` | 展示名称、说明 |
| `@localized` | table/singleton 字段绑定 language |
| `@dimension("region")` | table/singleton 字段绑定自定义维度 |
| `@expand` | 具体 data 字段的表格展开信息 |
| `@struct` | 无基类 sealed data 的结构体生成 |

继承使用冒号，abstract 不能实例化，sealed 不能派生。完整边界见[引用与多态](references-and-polymorphism.md)。

## 名称与文本

标识符使用 Unicode XID，首字符为 `_` 或 XID_Start，后续可含 XID_Continue，区分大小写。声明关键字、true/false、None/Some 等保留名称不能充当普通声明名。

字符串支持 `\n`、`\r`、`\t`、`\"`、`\\`，其他 Unicode 字符直接写。仅 `f"..."` 插值，默认值可引用当前对象字段路径；业务算术与条件应写在 check，不把它们当成默认值计算语言。写值细节见[CFD 语法](cfd-syntax.md)。
