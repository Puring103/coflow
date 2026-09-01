# CLI 命令

## 项目命令

```powershell
coflow init <dir>
coflow format [<project>] [--check]
coflow cft check <project>
coflow check <project>
coflow codegen <project>
coflow build <project>
coflow clean <project>
```

`check` 和 `cft check` 可使用 `--json` 输出诊断。`codegen` 和 `build` 只发布 `coflow.yaml` 中声明的目标语言源文件。

`format` 格式化项目配置的全部 `.cft` schema 和 `.cfd` data 文件。参数可以是项目目录或
`coflow.yaml`；省略时从当前目录查找项目。`--check` 不写文件，存在格式差异时返回失败，适合 CI。

## Schema 与服务

```powershell
coflow schema inspect <project> [--type TYPE] [--json]
coflow schema files <project> [--json]
Get-Content schema/main.cft | coflow schema write-file <project> --file schema/main.cft --check
coflow lsp <project>
coflow skill install -g
```

CLI 项目命令围绕 schema、CFD 数据检查和目标语言代码生成展开。
