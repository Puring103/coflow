# 程序视角

runtime 将 CFT schema 和 CFD lower 为不可变 `CfdDataModel`。C# generator 输出声明和 typed binding，`Coflow.Cfd.Runtime` 在游戏进程中直接读取 `SourceFiles`。

需要另一个语言时，实现 `coflow-codegen-api::CodeGenerator` 和目标语言 runtime binding；不添加新的数据源或导出接口。
