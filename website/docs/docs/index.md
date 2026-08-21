# Coflow CFD-only

Coflow 用 CFT 定义类型和规则，用 CFD 保存唯一的数据输入，再为多个目标语言生成类型代码。构建期执行 schema 编译、CFD 解析、引用检查和业务 check；运行时直接加载 CFD，不经过中间数据导出。

```text
coflow.yaml -> CFT -> CFD -> CfdDataModel -> check -> codegen
```

从 [快速开始](./guide/install.md) 开始，配置合同见 [项目配置](./reference/01-project-config.md)，C# runtime 见 [C# 代码生成](./reference/07-codegen/01-csharp.md)。
