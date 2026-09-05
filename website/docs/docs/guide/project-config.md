# 配置项目

```yaml
schema: schema/
data: data/
dimensions:
  language:
    variants: [en, zh]
    out_dir: data/dimensions/language
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

目录只扫描 `.cft` 和 `.cfd`。配置拒绝未知字段，产物仅由 `codegen` 声明。
