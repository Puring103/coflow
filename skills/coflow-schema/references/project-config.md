# 建模时配置输入和输出

coflow.yaml 位于项目根。相对路径都以它所在目录为基准，与命令调用目录无关。

```yaml
schema: schema/
data: data/
dimensions:
  language:
    display_name: 本地化
    dir: dimensions/language/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

schema 是必填项，可为文件、目录或路径列表；目录递归读取 .cft，共享全局声明。data 可省略，或使用 .cfd 文件/目录/列表；目录递归读取 CFD，新增文件自动纳入。

dimensions 可省略；维度名对应字段注解，dir 下 CFD 自动加入输入，不需重复写入 data。变体来自覆盖块，display_name 只控制展示；尚不存在的维度目录按空目录处理。只有 display_name 时仍可读取其他数据输入中的覆盖，编辑器新建推荐覆盖需 dir。

codegen 可省略、为单对象或列表；每项需要 language 和 dir，C# language 为 csharp，namespace 可省略，默认全局命名空间。输出目录由生成器管理，业务扩展放在目录外。

## 路径选择

- 公共类型可用 `../shared-schema/`，应用输出可用 `../Game/Generated/Config`。
- 配置使用相对路径；跨盘无法转成相对路径的位置不能使用。
- 输出目标互不重叠，并与 schema、data、维度输入分离。
- 不同维度目录互不重叠，且与 schema 输入分离。
- 同一来源只加载一次；文件顺序不决定覆盖优先级。

配置字段区分大小写，未知字段、重复字段和错误类型产生诊断。新增候选 .cft/.cfd 放在输入目录外，避免自动扫描。

## 检查实际输入

```sh
coflow project inspect --project <项目> --json
coflow project files --project <项目> --json
coflow check --project <项目> --rules --json
```

需要输入调整与源码一起发布时，用 apply files 的 add_input/remove_input，kind 为 schema/data；remove_input 不删除文件。维度和 codegen 等配置可直接编辑 YAML，随后校验。完整工作流见[结构变更](schema-migration.md)。
