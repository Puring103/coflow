# 总览

Coflow 将配置结构、数据和应用接入连接在一起：用 CFT 定义类型与校验规则，用 CFD 保存记录，用生成的 C# 类型在应用中加载和读取数据。项目配置统一组织输入文件与生成目录，本地化和平台差异通过维度覆盖表达。

- [快速开始](./guide/install.md)：用一个物品配置示例完成安装、项目配置、数据编写、代码生成与 C# 加载。
- [项目配置](./reference/01-project-config.md)：配置输入、维度目录和代码生成目标。
- [CFT](./reference/03-language/01-cft.md)：类型、声明、默认值与业务校验。
- [CFD](./reference/03-language/02-cfd.md)：记录、字段值、引用与覆盖。
- [C# 代码生成](./reference/07-codegen/01-csharp.md)：生成类型、加载数据与查询记录。
- [CLI 命令](./reference/08-cli.md)：查询、编辑、校验和生成的完整命令与参数。
- [本地化与维度](./reference/10-localization.md)：组织变体、检查覆盖并按需读取。
- [诊断](./reference/09-diagnostics/01-diagnostics.md)：读取诊断并根据[错误码](./reference/09-diagnostics/02-codes.md)定位问题。
- [编辑器插件](./reference/13-plugins.md)：安装项目插件，扩展视图、字段呈现、页面与命令。
