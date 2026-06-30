# 诊断

Coflow 诊断用于报告项目配置、CFT schema、数据源、DataModel、引用、业务校验、代码生成和产物写入中的结构化错误。

诊断不同于不可恢复的 CLI 错误。诊断表示 Coflow 已经进入可定位的项目检查阶段，可以给出 code、stage、message 和 source location；CLI 错误通常表示配置文件无法读取、命令参数无法解析等更早期失败。

完整错误码表见 [错误码索引](./diagnostics/codes.md)。

## 输出形式

人类可读诊断写到 stderr：

```text
----------------------------------------
[CFT-SCHEMA-006] [SCHEMA]
file    schema/main.cft
line    12
column  5
message
  unknown type `Missing`
```

表格诊断可能包含 sheet 和 cell：

```text
----------------------------------------
[CELL-TypeMismatch] [CELL]
file    data/items.xlsx
sheet   Item
cell    B2
message
  failed to parse `Item.level` cell: expected int
```

## JSON 输出

`coflow check --json` 和 `coflow cft check --json` 输出结构化诊断：

```json
{
  "diagnostics": [
    {
      "code": "CFT-SCHEMA-006",
      "stage": "SCHEMA",
      "severity": "error",
      "message": "unknown type `Missing`",
      "path": "schema/main.cft",
      "startLine": 0,
      "startCharacter": 14,
      "endLine": 0,
      "endCharacter": 21,
      "related": []
    }
  ]
}
```

JSON 中的行列位置是零基；human 输出显示为一基。

| 字段 | 说明 |
| --- | --- |
| `code` | 诊断码 |
| `stage` | 产生诊断的阶段 |
| `severity` | 严重级别，当前主要为 `error` |
| `message` | 错误说明 |
| `path` | 文件路径 |
| `sheet` | 表格 sheet 名 |
| `cell` | 表格 A1 单元格 |
| `startLine` / `startCharacter` | 起始位置 |
| `endLine` / `endCharacter` | 结束位置 |
| `related` | 相关位置，例如重复声明或重复 ID |

## 阶段

| Stage | 说明 |
| --- | --- |
| `CLI` | 命令级错误 |
| `PROJECT` | 项目配置和命令 preflight |
| `LEX` | CFT 词法 |
| `SYN` | CFT 语法 |
| `SCHEMA` | CFT schema 编译 |
| `TYPE` | CFT check 表达式静态类型检查 |
| `EXCEL` | Excel source、workbook、sheet、column、cell |
| `CSV` | CSV source 或写回 |
| `CFD` | CFD source、文本解析或写回 |
| `LARK` | 飞书/Lark source、鉴权、读取或写回 |
| `CELL` | 表格单元格值解析 |
| `DATA` | DataModel 构建 |
| `REF` | 记录引用和路径引用解析 |
| `CHECK` | CFT `check {}` 运行期校验 |
| `CODEGEN` | 代码生成 preflight |
| `ARTIFACT` | 输出目录、staging、commit、lockfile |
| `WRITE` | 通用 writer 能力错误 |

## 非阻塞收集规则

“非阻塞”表示 Coflow 会在不依赖无效中间数据的前提下继续收集诊断；不表示会在输入无效时生成产物。

| 阶段 | 收集行为 |
| --- | --- |
| 项目配置 | 独立字段会尽量一起报告 |
| CFT lex/syntax | 单文件内有限恢复，其他 schema 文件仍可继续 |
| CFT schema/type | 会尽量聚合 |
| Excel workbook/sheet | 其他 workbook/sheet 可继续 |
| Excel 表头 | 表头错误会跳过该 sheet 的数据行 |
| Cell parse | 有效表头下跨行收集 |
| CFD 文本 | 文件间继续，单文件内有限恢复 |
| DataModel | 子阶段内聚合；无效 model 阻塞引用和 check |
| 引用解析 | 子阶段内聚合；未解析引用阻塞 check |
| Check | 跨 block 和 record 聚合；硬运行期错误只停止当前 block |
| Codegen / artifact preflight | 尽量聚合 |

## 修复顺序

建议按阶段顺序修：

1. `CLI` / `PROJECT` / `DIM-CONFIG`：命令和项目配置。
2. `CFT-LEX` / `CFT-SYN` / `CFT-SCHEMA` / `CFT-TYPE`：CFT schema。
3. `EXCEL` / `CSV` / `LARK` / `CELL` / `CFD-TEXT`：数据源读取和文本解析。
4. `CFD-DATA` / `CFD-REF`：DataModel 和引用。
5. `CFD-CHECK`：业务规则。
6. `CODEGEN` / `ARTIFACT`：生成和输出目录。

前序阶段出错时，后续阶段可能不会运行。例如 DataModel 无效时，引用解析和 check 不会继续。
