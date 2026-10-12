# 程序员接入

在 coflow.yaml 中指定 schema、CFD 数据路径和代码生成目标，使用 `coflow check .` 校验声明与数据，使用 `coflow codegen .` 生成业务类型。

```yaml
schema: schema/
data: data/
codegen:
  - language: csharp
    dir: generated
    namespace: Game.Config
```

C# 项目引用 `runtimes/csharp/Coflow.Loader` 和生成文件：

```csharp
using Coflow.Loader;
using Game.Config;

var result = CoflowData.Load(new[] {
    new CfdSource("items.cfd", itemsText),
    new CfdSource("ja.cfd", localizationText),
});
if (result.Success) {
    var sword = result.Data.Table<Item>()["sword"];
    var name = sword.name.For("ja");
}
```

互相引用的记录一起加载，引用直接连接业务对象。整数与浮点使用 long/double，可选值使用 `Option<T>` 保留每层状态。应用选择文件、读取内容和处理加载诊断，编辑器执行 CFT 中的业务检查。

类型映射、partial 扩展、表与单例获取、枚举主键和加载结果见 [C# 参考](../reference/07-codegen/01-csharp.md)。
