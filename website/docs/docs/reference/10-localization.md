# 本地化与维度

维度是 CFD overlay 的 schema 级语义。配置声明变体和目录：

```yaml
dimensions:
  language:
    variants: [en, zh]
    out_dir: data/dimensions/language
```

每个变体文件都是 `.cfd`。runtime 在同一 generation 中加载基础记录和变体记录，check 对每个有效组合执行；代码 generator 根据 schema 生成目标语言的维度访问 API。

缺失变体、重复 key、未知字段和覆盖类型不匹配会在 check 阶段报告 source span。
