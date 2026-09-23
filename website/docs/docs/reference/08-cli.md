# CLI 命令

## 项目命令

```powershell
coflow init <dir>
coflow format [<project>] [--check]
coflow cft check <project>
coflow check <project>
coflow diff <project> [--json]
coflow codegen <project>
```

`check` 和 `cft check` 可使用 `--json` 输出诊断；指定 `--json` 后，项目打开或读取失败也输出 JSON 诊断。`codegen` 只发布 `coflow.yaml` 中声明的目标语言源文件，不执行 `check {}`；交付前先运行 `coflow check`。

`diff` 显示当前项目相对 Git `HEAD` 的源码与记录变化，包括新增、删除和修改。默认输出适合终端阅读的统一 Diff 与记录摘要；`--json` 输出结构化结果。当 `HEAD` 或当前项目无法形成完整数据模型时，命令仍提供源码变化和语义 Diff 诊断，但返回非零状态。

`format` 格式化项目配置的全部 `.cft` schema 和 `.cfd` data 文件。参数可以是项目目录或
`coflow.yaml`；省略时从当前目录查找项目。`--check` 不写文件，存在格式差异时返回失败，适合 CI。默认模式逐文件原子替换；如果处理过程中失败，此前已格式化的文件保持更新。

## Schema 与服务

```powershell
coflow schema inspect <project> [--type TYPE] [--json]
coflow schema files <project> [--json]
Get-Content schema/main.cft | coflow schema write-file <project> --file schema/main.cft --check
coflow lsp <project>
coflow skill install -g
```

`schema write-file --check` 先校验标准输入中的候选 schema；校验失败时不写文件并返回非零状态。`--dry-run` 始终不写文件。写入时以单文件原子替换。

CLI 项目命令围绕 schema、CFD 数据检查和目标语言代码生成展开。
