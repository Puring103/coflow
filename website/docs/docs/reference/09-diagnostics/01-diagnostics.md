# 诊断模型

每条诊断包含：

```text
code, stage, severity, message
primary: { location, message? }
related: [ label ]
contexts: [ structured context ]
```

位置类型包括项目配置 key path、CFT/CFD 文件 span 和 record field path。CLI 人类输出、JSON、LSP 和编辑器都渲染同一 canonical 诊断，不通过解析 message 重新推断位置。

解析预算、文件大小、递归深度、节点数和 record 数超限时，runtime 返回稳定错误码并放弃发布该 generation。
