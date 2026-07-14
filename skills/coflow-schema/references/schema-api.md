# Schema API

Schema API 是 `coflow-cft` 编译后的公开反射模型。它面向 codegen、loader、LSP、编辑器和其他宿主集成，不面向普通数据填写者。

如果只是编写 `.cft`，请看 [CFT 语法参考](./03-language/01-cft.md)。如果要在 Provider 或工具中读取编译后的 schema，本页说明应依赖哪些稳定概念。

## 范围

Schema API 只描述完成解析和编译后的 CFT 结构：

- 不负责发现 `coflow.yaml`。
- 不负责解析项目相对路径。
- 不重新暴露原始 AST。
- 不加载数据源。
- 不构建 DataModel。
- 不运行 `check {}` 的运行期求值。

项目级宿主通过 `coflow-runtime::ProjectRuntime` 获取已编译 schema；低层工具收集 `CftFile` 后调用 `parse_modules` 和 `build_schema`。

## 核心入口

`CftModuleSet` 是不可变的解析结果，`CftSchema` 是唯一的编译后语义 schema：

```rust
let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
let schema = build_schema(&modules, &CftDimensions::default())?;
```

常用读取能力包括：

| API | 用途 |
| --- | --- |
| `CftModuleSet::files()` | 遍历模块和源文本 |
| `CftModuleSet::module(module)` | 读取已解析的 module AST |
| `all_types()` | 遍历所有 type |
| `all_enums()` | 遍历所有 enum |
| `resolve_type(name)` | 按名称查找 type |
| `resolve_enum(name)` | 按名称查找 enum |
| `resolve_const(name)` | 按名称查找 const |
| `has_type(name)` / `has_enum(name)` | 快速判断名称存在性 |
| `is_assignable(actual, expected)` | 判断类型可赋值关系 |
| `enum_variant_value(enum, variant)` | 查找 enum variant 底层值 |

## Type 反射

编译后的 type 同时保留自身字段和继承展开字段：

| 字段 | 说明 |
| --- | --- |
| `name` | type 名 |
| `fields` | 当前 type 自己声明的字段 |
| `all_fields` | 继承展开后的有效字段，父类字段在前 |
| `annotations` | type 上的注解 |
| `is_abstract` | 是否为 abstract type |
| `is_sealed` | 是否为 sealed type |
| `is_singleton` | 是否标记 `@singleton` |
| `check` | 编译后的 check block |

Loader、codegen 和编辑器应优先使用 `all_fields`。这样继承字段顺序稳定，不需要每个消费者重新展开继承链。

## Field 反射

字段反射提供名称、类型、默认值、注解和源位置。

| 信息 | 用途 |
| --- | --- |
| 字段名 | 表头映射、代码成员名、编辑器列名 |
| `ty` / `ty_ref` | 类型映射和 schema-guided 解析 |
| `default` | 缺省值应用和代码生成 |
| `annotations` | `@expand`、`@localized` 等字段行为 |
| `span` / `module` | 诊断、跳转、hover 定位 |

消费者不应从字段类型字符串重新解析类型；应使用已解析的类型引用。消费者也不应从源文本重新解释默认值。

## Enum 与 Const

Enum 反射包含：

- enum 名。
- variant 名。
- variant 底层整数值。
- enum / variant 注解。
- 是否为 `@flag` 枚举。

`@flag` 枚举允许运算得到没有显式 variant 名的组合值。运行时 enum value 因此会携带底层整数值，并且 variant 名可能为空。

Const 反射包含编译期常量值。常量可以用于默认值和 `check {}` 表达式，但不能表示运行期字段值。

## 注解消费约定

Schema API 会保留编译后的注解。常见消费方式：

| 注解 | 主要消费者 |
| --- | --- |
| `@idAsEnum` | codegen、导出和 enum lock |
| `@singleton` | DataModel、codegen |
| `@localized` | engine 维度注入、codegen |
| `@expand` | 表格 loader / writer |
| `@flag` | CFT type check、DataModel、codegen |

记录引用不是注解语义。消费者应读取字段类型中的 `Ref` / `&Type` 结构：DataModel、cell parser、CFD loader、writer 和 codegen 都应按 schema type 决定引用或内联对象行为。

注解目标、参数数量和参数类型由 schema 编译阶段检查。Provider 不应重复做注解语法校验。

## 消费者约定

工具和 Provider 应遵守这些规则：

- 使用 runtime generation 中的已编译 `CftSchema`，不重新读取或解析 AST。
- 使用 `all_fields` 处理继承字段顺序。
- 使用类型反射处理字段类型，不从字符串手写解析。
- 使用 schema 中的 `span` 和 `module` 生成定位。
- 使用编译后的默认值，不从源文本重新解释。
- 使用 `is_assignable` 判断多态可赋值关系。
- 不在 Provider 内执行业务 check；check 由 engine / checker 统一运行。

## 与项目运行时的关系

Schema API 是低层能力。完整项目运行时还会继续做：

1. 读取 `coflow.yaml`。
2. 发现并排序 schema 文件。
3. 收集 CFT files。
4. 解析 modules 并编译 schema。
5. 构建维度合成 type。
6. 解析和加载 sources。
7. 构建 DataModel。
8. 运行 check。

这些项目级步骤由 `coflow-project` 和 `coflow-runtime` 负责。Provider 和 codegen 通常只消费已经准备好的 schema。
