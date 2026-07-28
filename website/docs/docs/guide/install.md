# 安装

Coflow 提供 CLI 和 Windows 可视化编辑器。CLI 是项目校验、构建和 CI 接入的权威入口。

## 安装 CLI

本机已安装 Rust toolchain 时，可以直接从 GitHub 安装：

```powershell
cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --help
```

也可从 [最新 Release](https://github.com/Puring103/coflow/releases/latest) 下载预编译包。Windows 完整安装包包含 CLI 和编辑器，CLI-only 包只安装命令行工具。

## 验证项目

在仓库根目录运行自带的 RPG 示例：

```powershell
coflow check examples/rpg
coflow build examples/rpg
```

`check` 只读取并验证项目；`build` 在验证通过后发布配置中声明的数据和代码产物。详细参数见 [CLI 命令参考](../reference/08-cli.md)。

## AI Agent Skills

CLI 内置 Coflow skills，可安装到当前项目或当前用户：

```powershell
coflow skill install .
coflow skill install -g
```

完整说明见 [AI Agent Skills](ai-agent.md)。
