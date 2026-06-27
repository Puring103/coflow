---
name: coflow-cft-cfd-authoring
description: "Coflow CFT schema 与 CFD 文本配置编写指南：当用户需要设计游戏配置 schema、编写或修改 .cft/.cfd、建模类型/枚举/默认值/check、@idAsEnum、@singleton、@localized、维度/变体、记录引用、数组/字典/继承/多态/覆盖语法时使用。"
---

# Coflow CFT CFD Authoring

使用本 skill 编写 Coflow 的 CFT schema 和 CFD 文本数据。CFT 描述配置数据结构、默认值、引用和校验；CFD 适合写复杂嵌套对象、数组、字典、继承对象和覆盖模板。

## CFT 编写原则

1. 用 `type` 定义顶层记录结构，用字段类型表达数据形状。
2. 用 `enum` 表示稳定离散值；需要 data-driven id enum 时使用 `@idAsEnum(Name)` 并声明空 enum。
3. 给字段设置合理默认值，减少数据表重复填写。
4. 用 nullable `T?` 表达可缺省引用或可空值。
5. 用 `check {}` 表达业务约束，例如数值范围、数组唯一性、引用集合规则、条件约束和多态类型判断。
6. 抽象基类用于多态字段，具体可实例化类型用普通或 sealed type。
7. 需要语言/变体值时，在字段上使用 `@localized`，并要求 `coflow.yaml` 配置 `dimensions.language`。

## CFD 编写原则

1. 顶层记录写成 `key: Type { ... }`；`key` 承担 id 语义，不要在记录块中写 `id` 字段。
2. 字段使用 `name: value`，字段和数组/字典条目用逗号分隔，允许尾逗号。
3. 对象引用优先写 `@Type.key`；同类型对象字段可用 `&key` 简写。
4. 需要路径访问时写 `@Type.key.field[index]`，不要使用旧的 `@key`。
5. 对象和字典覆盖使用 `...source`，后出现的值覆盖前面的值，本地字段覆盖 spread。
6. CFD 没有表头；字段变化后可用 `coflow data sync-header --file x.cfd --type Type` 重写顶层字段。

## 校验

修改 `.cft` 时优先用命令写入并检查：

```powershell
coflow schema write-file <project> --file schema/main.cft --stdin --check
```

命令不可用或用户明确要求直接编辑时，修改后运行：

```powershell
coflow schema inspect <project>
coflow check <project>
```

重写复杂 `.cfd` 文件时优先用命令：

```powershell
coflow data write-file <project> --file data/items.cfd --stdin --check
```

该命令只用于配置内本地 CFD source 覆盖的 `.cfd`；CSV/XLSX 不用整文件写入。

修改 `.cfd` 或表格数据后运行：

```powershell
coflow check <project>
```

## 何时读取 reference

- 需要 CFT/CFD 语法示例、常量、枚举、`@flag`、字段类型、默认值、nullable、check、注解目标、`@idAsEnum`、`@localized`、维度/变体或表格关系细节时，读取 `references/cft-cfd-syntax.md`。
