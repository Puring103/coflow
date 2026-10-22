# 安装与快速开始

从 [GitHub Releases](https://github.com/Puring103/coflow/releases/latest) 下载 Windows 安装程序或 macOS 应用及 CLI。已有 Rust 工具链时，也可以从源码安装 CLI：

```powershell
cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --help
```

创建项目目录与配置：

```powershell
coflow init my-config
```

在 `my-config/schema/items.cft` 中定义记录类型：

```cft
table Item {
  name: string;
  price: int = 0;
  check Valid { price >= 0: "价格不能为负数"; }
}
```

在 `my-config/data/items.cfd` 中填写数据：

```cfd
sword: Item { name: "铁剑", price: 100 }
```

运行构建，检查声明、数据和业务规则，并生成 C# 文件：

```powershell
coflow build --project my-config
```

默认输出目录为 `my-config/generated/csharp`。将生成文件与 `runtimes/csharp/Coflow.Loader` 引入应用，通过 `CoflowData.Load` 加载 CFD 内容。接下来可以阅读[配置项目](./project-config.md)、[Showcase 示例](./examples.md)和 [C# 接入](../reference/07-codegen/01-csharp.md)。
