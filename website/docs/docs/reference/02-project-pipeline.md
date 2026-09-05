# 项目流水线

项目执行只有一条输入链路：

```text
coflow.yaml -> CFT compiler -> fixed CFD loader -> parser/lowerer
            -> CfdDataModel -> checks -> CodegenInput -> language source files
```

runtime 负责文件发现、文本读取、schema/data/check 诊断和不可变 generation。CLI、LSP 和编辑器共享这条 pipeline。

## 命令

- `coflow cft check` 只编译 schema。
- `coflow check` 加载全部 CFD、执行引用解析和 check，不写产物。
- `coflow codegen` 在 check 成功后按每个 `codegen` target 发布源文件。
- `coflow build` 等价于 check 加全部 codegen target 的原子发布。

任意阶段失败都不会替换上一次成功的 generation 或代码目录。发布前会校验相对路径、重复文件和内容清单。
