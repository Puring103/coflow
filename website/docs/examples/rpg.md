# RPG 示例

`examples/rpg` 是一个同时使用多文件 CFT schema、Excel 和 CFD 的完整项目。它用于验证跨源引用、复杂 check、JSON 导出和 C# codegen。

## 目录

```text
examples/rpg/
  coflow.yaml
  schema/          # 按业务域拆分的 CFT
  data/rpg.xlsx    # Item、Equipment、Skill、Buff、Monster、Text
  data/cfd/        # DropTable、Stage、Quest、Shop 等嵌套数据
  generated/       # build 成功后的稳定输出
```

Excel 中的 item/equipment 可以引用 CFD 中的 stage，CFD 中的进度数据也可以引用 Excel 中的 item、monster、skill 和 buff。这些数据加载后进入同一个 data model。

## 运行

```powershell
coflow cft check examples/rpg
coflow check examples/rpg
coflow build examples/rpg
```

构建成功后检查：

```text
examples/rpg/generated/data
examples/rpg/generated/csharp
examples/rpg/.coflow/artifacts/active.json
```

## 实验一次诊断

临时把 Excel 或 CFD 中的某个引用 key 改为不存在的值，然后运行 `coflow check examples/rpg`。诊断应指出 source 位置、record 和字段路径。恢复原值后再次检查应通过。

如果修改了表格示例数据，可使用 `node examples/rpg/scripts/build-rpg-workbook.mjs` 重建 workbook。
