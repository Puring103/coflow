# 快速开始

这篇教程使用一个物品配置示例：定义铁剑的名称与价格，生成 C# 类型，然后在已有 C# 应用中加载 CFD 并读取物品信息。

## 1. 安装

从 [GitHub Releases](https://github.com/Puring103/coflow/releases/latest) 下载适合系统的 CLI，将可执行文件所在目录加入 `PATH`。也可以在安装 Rust 工具链后从源码安装：

```sh
cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --version
coflow --help
```

## 2. 创建并配置项目

选择配置项目的保存位置，运行：

```sh
coflow init item-config
```

将 `item-config/coflow.yaml` 设置为：

```yaml
schema: schema/
data: data/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

`schema` 指向 CFT 类型文件，`data` 指向 CFD 数据，`codegen` 指定 C# 文件的生成位置和命名空间。这里的路径都相对 `item-config`，目录中的输入文件会递归读取。

## 3. 用 CFT 定义物品

创建 `item-config/schema/items.cft`：

```cft
table Item {
  name: string;
  price: int = 0;
  check ValidPrice {
    price >= 0: "价格不能为负数";
  }
}
```

`table Item` 表示有 ID 的物品记录。`name` 必填，`price` 省略时为 `0`；`ValidPrice` 要求价格非负。

## 4. 编写 CFD 数据

创建 `item-config/data/items.cfd`：

```cfd
sword: Item {
  name: "铁剑",
  price: 100,
}
```

`sword` 是记录 ID，`Item` 是 CFT 中声明的类型。字段以逗号分隔。

## 5. 生成 C# 代码

```sh
coflow build --project item-config
```

`build` 检查 CFT、CFD 和业务规则，全部通过后将 C# 源码生成到 `item-config/generated/csharp`。例如将价格改为负数会产生校验诊断，修正后再构建。

单独执行校验或代码生成时可使用：

```sh
coflow check --project item-config --rules
coflow codegen --project item-config
```

`codegen` 检查类型与数据；交付前需要执行全部业务规则时使用 `build`。

## 6. 在 C# 中加载

在已有应用中引用 `Coflow.Loader` 和 `item-config/generated/csharp` 下的生成源码。加载器与 CLI 使用同一发布版本，应用的资源系统负责读取 CFD 文本并传入加载器：

```csharp
using Coflow.Loader;
using Game.Config;

// itemsText 是前面 data/items.cfd 的文本内容。
var result = CoflowData.Load(new[] {
    new CfdSource("data/items.cfd", itemsText),
});

// 先确认加载成功，再读取业务数据。
if (!result.Success) {
    foreach (var diagnostic in result.Diagnostics)
        System.Console.Error.WriteLine(diagnostic);
    return;
}

var sword = result.Data.Table<Item>()["sword"];
var name = sword.name;   // 铁剑
var price = sword.price; // 100
```

修改 CFD 后重新加载即可读取新值；修改 CFT 结构后重新生成并编译源码。完整类型映射、引用与维度访问见 [C# 数据访问](../reference/07-codegen/01-csharp.md)。
