# 从零创建完整项目

先确定实体、关键业务值、引用关系和校验要求。仅需要演示时使用下面的样例值；业务项目中的名称、价格和限制应来自用户要求。

## 初始化

```sh
coflow init <新项目目录>
```

init 创建 coflow.yaml 和输入、生成目录，不创建业务类型或数据；已有配置时不会覆盖。后续命令显式指定这个项目。

## 配套示例

[minimal-project](../examples/minimal-project/coflow.yaml) 包含相互匹配的配置、类型和数据，可整体复制到新的演示目录，或在刚初始化的空项目中按其内容创建文件：

```text
coflow.yaml
schema/main.cft
data/items.cfd
```

- [类型](../examples/minimal-project/schema/main.cft)：Item、内联 Stats、Quality、singleton Settings；必填 name/price、Option next、集合、语言和地区字段、数据规则与顶层规则。
- [数据](../examples/minimal-project/data/items.cfd)：sword 和 shield、Settings 同名记录、日语覆盖；必填字段和引用闭合。
- [配置](../examples/minimal-project/coflow.yaml)：输入目录、C# 命名空间和输出目录。覆盖与基础记录放在同一个输入文件中，无需额外维度目录。

示例不是要追加到已有业务项目的固定 Schema。改造成业务项目时同时调整声明、记录、覆盖和规则，特别注意 singleton 必须有同名记录。

## 验证与读取

```sh
coflow check --project <新项目目录> --rules --json
coflow context --project <新项目目录> --type Item --json
coflow data get --project <新项目目录> --type Item --id sword --view both --json
coflow dimension get --project <新项目目录> --type Item --id sword --field name --dimension language --variant ja --json
```

原样示例应完整通过检查，Item 表有两条记录；sword.price 为 90，next 指向 shield，基础 name 为“铁剑”，日语有效 name 为“剣”。Settings.start 指向 sword。

需要 C# 产物时运行 `coflow build --project <新项目目录> --json`，会生成到配置目录。应用引用 Loader 与产物是后续接入任务；build 成功不等于应用已完成集成。

## 常见调整

只维护配置而不生成代码时移除 codegen 配置；没有基础数据的纯类型项目可省略 data，但声明 singleton 或依赖记录的规则后需提供相应记录。目录和输入选择见[项目配置](project-config.md)。

新建多份关联源码可直接创建后 check --rules，或使用文件候选一次发布。已有项目采用[结构变更](schema-migration.md)，避免先写新类型导致旧数据暂时不合法。
