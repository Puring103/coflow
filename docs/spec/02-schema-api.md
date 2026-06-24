# Schema API

本文档描述 `coflow-cft` 编译后暴露的公开 Rust schema 反射模型。它面向
codegen、Excel loader、LSP 和其他宿主集成使用。

---

## 范围

Schema API 只描述已经完成 CFT 解析和编译后的反射结构：

- 不负责项目路径发现或文件读取。
- 不重新暴露原始 AST。
- 不执行数据加载、data model 构建或 `check {}` 运行期求值。

宿主应通过 `CftContainer` 注册模块并完成 `compile()`，再读取 schema 反射信息。

---

## 稳定概念

- `CftContainer` 持有已编译模块，并暴露全部 const、enum 和 type。
- `CftSchemaModule` 按 module 分组定义。
- `CftSchemaType.fields` 是当前 type 自己声明的字段。
- `CftSchemaType.all_fields` 是继承展开后的有效字段列表，父类字段在前。
- `CftSchemaField.ty_ref` 是已解析的公开类型引用。
- `span` 和 `module` 标识源代码位置，用于诊断、LSP 和跳转。
- `check` 保存编译后的 check expression block。
- `default` 保存编译后的默认值。
- `CftSchemaEnumVariant.annotations` 保存 enum variant 级 `@display` 和
  `@deprecated`。
- `CftSchemaType.is_singleton` 在 type 标注了 `@singleton` 时为 `true`。
- `CftSchemaField.is_localized` 在字段标注了 `@localized` 时为 `true`；
  `CftSchemaField.localization_bucket` 为该字段最终归属的 bucket 名（无参 `@localized` 时为字段所属 type 名，带参时为参数值；非本地化字段为 `None`）。

---

## 消费者约定

codegen crate、loader 和编辑器工具应消费 schema API，而不是重新读取 AST 或
解析源文本：

- 类型映射应基于 `ty_ref`，避免从字段类型字符串重新解析。
- 字段顺序应使用 `all_fields`，保证继承字段顺序稳定。
- 诊断和编辑器定位应使用 `module` 与 `span`。
- 默认值应使用 `default`，不要从源文本重新解释。
- check 相关工具应消费编译后的 check 反射树。

---

## API 摘要

完整结构定义以 Rust 代码为准，核心入口如下：

```rust
impl CftContainer {
    pub fn new() -> Self;

    pub fn add_module(
        &mut self,
        id: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), CftDiagnostics>;

    pub fn compile(&mut self) -> Result<(), CftDiagnostics>;

    pub fn schema(&self, id: &ModuleId) -> Option<&CftSchemaModule>;
    pub fn resolve_type(&self, name: &str) -> Option<&CftSchemaType>;
    pub fn resolve_enum(&self, name: &str) -> Option<&CftSchemaEnum>;
    pub fn resolve_const(&self, name: &str) -> Option<&CftSchemaConst>;

    pub fn module_ids(&self) -> impl Iterator<Item = &ModuleId>;
    pub fn all_types(&self) -> impl Iterator<Item = &CftSchemaType>;
    pub fn all_enums(&self) -> impl Iterator<Item = &CftSchemaEnum>;
    pub fn has_type(&self, name: &str) -> bool;
    pub fn has_enum(&self, name: &str) -> bool;
    pub fn source(&self, id: &ModuleId) -> Option<&str>;
    pub fn is_assignable(&self, actual_type: &str, expected_type: &str) -> bool;
    pub fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64>;
}
```

`add_module` 成功会让此前发布的 schema 视图失效；失败的 `add_module` 不改变
容器，也不废弃已发布 schema。失败的 `compile` 不发布新 schema。
