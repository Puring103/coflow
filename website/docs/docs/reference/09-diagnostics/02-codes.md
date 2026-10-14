# 错误码索引

错误码用于识别诊断类别。源码位置、记录坐标和字段路径由诊断结构提供。

使用 `coflow explain <code> --json` 查询含义、触发条件、修复方向与进一步查询入口；`check --explain` 和 `schema check --explain` 返回当前诊断的解释。

```bash
coflow explain CFT-SCHEMA-006 --json
coflow explain REF-001 --json
```

## CFT 声明与检查表达式

| 错误码 | 含义 |
| --- | --- |
| `CFT-LEX-001` | 非法字符 |
| `CFT-LEX-002` | 字符串转义无效 |
| `CFT-LEX-003` | 字符串未结束 |
| `CFT-LEX-004` | 整数字面量无效 |
| `CFT-LEX-005` | 浮点数字面量无效 |
| `CFT-SYN-001` | 意外的语法单元 |
| `CFT-SYN-002` | 源码提前结束 |
| `CFT-SYN-003` | 缺少标识符 |
| `CFT-SYN-004` | 缺少指定符号 |
| `CFT-SYN-005` | 顶层声明无效 |
| `CFT-SYN-006` | 链式比较无效 |
| `CFT-SYN-007` | check 块位置无效 |
| `CFT-SYN-008` | 注解语法无效 |
| `CFT-SYN-009` | 检查语句无效 |
| `CFT-SYN-010` | 检查块重复 |
| `CFT-SYN-011` | 语法结构超出限制 |
| `CFT-SCHEMA-001` | 模块重复 |
| `CFT-SCHEMA-002` | 全局名称重复 |
| `CFT-SCHEMA-003` | 字段名称重复 |
| `CFT-SCHEMA-004` | 枚举成员重复 |
| `CFT-SCHEMA-005` | 枚举数值重复 |
| `CFT-SCHEMA-006` | 类型未声明 |
| `CFT-SCHEMA-007` | 继承目标不是类型 |
| `CFT-SCHEMA-008` | 常量未声明 |
| `CFT-SCHEMA-009` | 类型继承成环 |
| `CFT-SCHEMA-010` | 继承 sealed 类型 |
| `CFT-SCHEMA-011` | 继承字段重名 |
| `CFT-SCHEMA-012` | 类型修饰符冲突 |
| `CFT-SCHEMA-014` | 字典键类型无效 |
| `CFT-SCHEMA-015` | 默认值表达式无效 |
| `CFT-SCHEMA-016` | 默认值类型不匹配 |
| `CFT-SCHEMA-017` | 默认值引用字段 |
| `CFT-SCHEMA-018` | 枚举数值序列无效 |
| `CFT-SCHEMA-019` | flag 枚举数值无效 |
| `CFT-SCHEMA-020` | 注解未定义 |
| `CFT-SCHEMA-021` | 注解重复 |
| `CFT-SCHEMA-022` | 注解没有目标 |
| `CFT-SCHEMA-023` | 注解目标无效 |
| `CFT-SCHEMA-024` | 注解参数无效 |
| `CFT-SCHEMA-025` | 注解字段类型无效 |
| `CFT-SCHEMA-026` | struct 缺少 sealed |
| `CFT-SCHEMA-027` | idAsEnum 目标枚举非空 |
| `CFT-SCHEMA-028` | 枚举成员目标错误 |
| `CFT-SCHEMA-029` | 枚举成员不存在 |
| `CFT-SCHEMA-030` | 常量值无效 |
| `CFT-SCHEMA-031` | 使用了保留标识符 |
| `CFT-SCHEMA-034` | localized 目标无效 |
| `CFT-SCHEMA-035` | 本地化名称无效 |
| `CFT-SCHEMA-032` | dimension 目标无效 |
| `CFT-SCHEMA-033` | 维度名称无效 |
| `CFT-SCHEMA-036` | 抽象类型不能 singleton |
| `CFT-SCHEMA-037` | singleton 与 idAsEnum 冲突 |
| `CFT-SCHEMA-038` | Schema 结构超出限制 |
| `CFT-SCHEMA-039` | 项目级检查重名 |
| `CFT-SCHEMA-040` | 类型别名成环 |
| `CFT-SCHEMA-042` | 必填对象依赖成环 |
| `CFT-SCHEMA-043` | 默认值物化成环 |
| `CFT-TYPE-001` | 值名称未定义 |
| `CFT-TYPE-002` | 字段未定义 |
| `CFT-TYPE-003` | 检查中的枚举成员不存在 |
| `CFT-TYPE-004` | 检查中的枚举目标错误 |
| `CFT-TYPE-005` | 运算符类型不匹配 |
| `CFT-TYPE-006` | 比较类型不匹配 |
| `CFT-TYPE-007` | 检查条件不是 bool |
| `CFT-TYPE-008` | 检查函数未定义 |
| `CFT-TYPE-009` | 函数参数数量错误 |
| `CFT-TYPE-010` | 函数参数类型错误 |
| `CFT-TYPE-011` | 非对象不能访问字段 |
| `CFT-TYPE-012` | 目标不能使用下标 |
| `CFT-TYPE-013` | 下标类型不匹配 |
| `CFT-TYPE-014` | 类型判断无效 |
| `CFT-TYPE-015` | 量词目标不是集合 |
| `CFT-TYPE-016` | unique 元素类型无效 |
| `CFT-TYPE-017` | 位运算类型无效 |
| `CFT-TYPE-018` | 位移要求整数 |
| `CFT-TYPE-019` | 正则模式不是字面量 |
| `CFT-TYPE-020` | 正则模式无效 |
| `CFT-TYPE-021` | 量词绑定无效 |
| `CFT-TYPE-022` | 记录集合查询无效 |

## 数据模型、引用与规则执行

| 错误码 | 含义 |
| --- | --- |
| `DATA-001` | 记录类型未定义 |
| `DATA-002` | 记录类型为抽象类型 |
| `DATA-003` | 对象类型缺失 |
| `DATA-004` | 对象类型不匹配 |
| `DATA-005` | 数据字段未声明 |
| `DATA-006` | 必填字段缺失 |
| `DATA-007` | 数据值类型不匹配 |
| `DATA-008` | 枚举值无效 |
| `DATA-009` | 字典键重复 |
| `DATA-010` | 记录身份信息缺失 |
| `DATA-011` | 记录键重复 |
| `DATA-012` | 多态记录键重复 |
| `DATA-013` | 记录键无效 |
| `DATA-014` | 值依赖成环 |
| `DATA-015` | singleton 记录数量无效 |
| `DATA-016` | singleton 记录键无效 |
| `DATA-017` | singleton 记录键冲突 |
| `DATA-018` | 数据结构超出限制 |
| `REF-001` | 引用目标不存在 |
| `REF-002` | 引用目标类型不匹配 |
| `CHECK-001` | 业务规则未通过 |
| `CHECK-002` | 规则求值类型错误 |
| `CHECK-004` | 规则数组下标越界 |
| `CHECK-005` | 规则字典键不存在 |
| `CHECK-006` | 空集合不能求极值 |
| `CHECK-007` | 规则比较未通过 |
| `CHECK-008` | 规则布尔条件未通过 |
| `CHECK-009` | 规则否定条件未通过 |
| `CHECK-010` | 规则合取条件未通过 |
| `CHECK-011` | 规则析取条件未通过 |
| `CHECK-012` | 规则类型判断未通过 |
| `CHECK-014` | 规则包含条件未通过 |
| `CHECK-015` | 规则唯一性条件未通过 |
| `CHECK-016` | 规则模式匹配未通过 |
| `CHECK-017` | any 规则未通过 |
| `CHECK-018` | none 规则未通过 |
| `CHECK-019` | all 规则未通过 |
| `CHECK-020` | 规则执行预算耗尽 |
