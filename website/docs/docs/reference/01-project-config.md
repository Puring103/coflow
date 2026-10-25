# 项目配置

`coflow.yaml` 是项目入口，用于选择 CFT 类型文件、CFD 数据、维度覆盖目录和代码生成目标。`coflow init <目录>` 创建初始项目；命令使用 `--project <目录或 coflow.yaml>` 选择项目，省略时从当前目录向父目录查找。

## 完整配置

```yaml
schema:
  - schema/
  - shared/common.cft
data:
  - data/
  - overlays/base.cfd
dimensions:
  language:
    display_name: 本地化
    dir: dimensions/language/
  platform:
    display_name: 平台
    dir: dimensions/platform/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

只填写实际使用的输入路径。只有 `schema` 是必填顶层字段；省略 `data`、`dimensions` 或 `codegen` 时，分别表示没有基础数据输入、维度目录配置或代码生成目标。

## 字段一览

| 字段 | 类型 | 必填 / 默认 | 如何配置 |
| --- | --- | --- | --- |
| `schema` | 路径字符串或路径列表 | 必填 | `.cft` 文件或目录；目录递归读取 `.cft` |
| `data` | 路径字符串或路径列表 | 可选，空列表 | `.cfd` 文件或目录；目录递归读取 `.cfd` |
| `dimensions` | 维度名到配置对象的映射 | 可选，空映射 | 为 CFT 中使用的维度设置显示名称与覆盖目录 |
| `dimensions.<名称>.display_name` | 字符串 | 可选 | 编辑器中显示的名称；省略时使用内置名称或维度原名 |
| `dimensions.<名称>.dir` | 目录路径 | 可选 | 递归读取覆盖 CFD，也作为编辑器新建覆盖文件的目录 |
| `codegen` | 目标对象或目标对象列表 | 可选，空列表 | 多个目标使用列表 |
| `codegen[].language` | 字符串 | 每个目标必填 | C# 使用 `csharp` |
| `codegen[].dir` | 目录路径 | 每个目标必填 | 保存生成源码的专用目录 |
| `codegen[].namespace` | 字符串 | C# 可选，默认全局命名空间 | 例如 `Game.Config`；空字符串也表示全局命名空间 |

配置字段名区分大小写；未知字段、重复字段及错误的值类型会产生配置诊断。

## 类型输入：schema

一个文件或一个目录直接使用字符串；多个输入使用列表：

```yaml
schema: schema/
```

```yaml
schema:
  - schema/items.cft
  - ../shared-schema/
```

目录会递归读取 `.cft` 文件。所有输入文件中的声明共同组成项目的全局类型集合，可直接相互使用；同名声明产生诊断。项目外的公共类型通过 `../` 相对路径引用。

## 数据输入：data

```yaml
data:
  - data/
  - events/current.cfd
```

目录只递归读取 `.cfd`，忽略其他扩展名。显式文件路径必须是 CFD 文件；每个列表项是路径字符串。记录可跨文件引用，文件顺序不表示覆盖优先级。同一来源只加载一次。

记录可以按业务模块分文件；应用加载时需一起提供互相引用的数据文件。目录中新加入的 CFD 会纳入项目，无需逐一修改配置。命令 `coflow project files --kind data` 可查看实际输入。

## 维度：dimensions

维度名对应 CFT 的 `@dimension("名称")`；`@localized` 使用 `language`。在 CFT 上绑定字段，在 CFD 中用 `language[ja] { ... }` 等块提供变体。YAML 用于配置文件组织与展示，变体从 CFD 覆盖块发现。

```yaml
dimensions:
  language:
    display_name: 本地化
    dir: dimensions/language/
```

维度目录中的 `.cfd` 自动纳入输入，无需在 `data` 中重复声明。目录尚不存在时按空目录处理，编辑器首次保存覆盖时创建。只配置 `display_name` 时仍可读取已纳入数据输入的覆盖；使用编辑器新增推荐覆盖文件前需配置 `dir`。

编辑器保留基础文件相对项目根目录的完整路径：`data/equipment/items.cfd` 的 language 覆盖保存到 `dimensions/language/data/equipment/items.cfd`。同一基础文件、同一维度的记录、字段和变体集中保存；已有覆盖仍写回原来源。

不同维度目录互不重叠，并与 schema 输入分离。完整覆盖与回退语义见[本地化与维度](./10-localization.md)。

## 代码生成：codegen

```yaml
codegen:
  - language: csharp
    dir: ../Game/Generated/Config
    namespace: Game.Config
```

`language` 选择生成目标；C# 的 `namespace` 由点分隔的合法 C# 标识符组成。省略或设为空字符串时，生成类型位于全局命名空间。

单个目标也可以使用对象：

```yaml
codegen:
  language: csharp
  dir: generated/csharp
```

输出目录由代码生成维护，业务扩展代码放在该目录外。多个目标目录必须互不重叠，也必须与 schema、data 和维度输入目录分离。项目外输出可配置为 `../Game/Generated/Config`，适合将生成源码直接放入应用项目。

运行 `coflow codegen` 生成或更新源码；`coflow codegen --dry-run` 预览，`coflow codegen --check` 检查产物是否与当前项目一致。`coflow build` 在全部业务校验通过后生成代码。数据加载方式见 [C#](./07-codegen/01-csharp.md)。

## 路径与检查

所有相对路径都以 `coflow.yaml` 所在目录为基准，与执行命令的当前目录无关。可以读取项目外的输入或生成到项目外目录；配置路径统一使用相对路径，外部位置使用 `../`。读取已有绝对路径时转换为相对路径，下次保存配置时写回；无法转换为相对路径的跨盘位置会报错。

```sh
coflow project inspect --project . --json
coflow project files --project . --json
coflow check --project . --rules
```

第一条查看配置，第二条核对实际文件，第三条校验声明、数据和业务规则。配置错误会附带文件位置或配置项路径，详见[诊断](./09-diagnostics/01-diagnostics.md)。
