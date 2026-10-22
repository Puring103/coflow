# Schema API

Schema 表示 CFT 声明检查后的只读类型信息，包括类型、继承、字段、默认值、枚举、常量、类型别名、维度和检查规则。名称在项目中全局唯一，来源位置用于诊断和源码导航。

工具可以查询字段的声明类型、具体子类型、记录引用目标、Option 层、集合元素、枚举成员、维度绑定及展示注解。检查规则由 CLI 或编辑器执行，代码生成使用声明中的数据类型与默认值。

命令行使用 `coflow schema inspect --type Item --json` 查询类型，使用 `coflow schema symbols --json` 查询符号，使用 `coflow schema refs --symbol Item.price --json` 查询使用位置。业务规则可通过 `coflow rule list --json` 和 `coflow rule inspect --rule ItemPrices --json` 查看；用 `coflow describe <命令> --json` 查看参数，`--schema output` 按需展开返回结构。

应用通过生成的 C# 类型读取 CFD 加载结果。C# 接入见 [代码生成与加载](./07-codegen/01-csharp.md)，声明语法见 [CFT](./03-language/01-cft.md)。
