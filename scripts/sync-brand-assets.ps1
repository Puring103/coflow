$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$logoPath = Join-Path $repoRoot 'assets/coflow-logo.svg'
$publicPath = Join-Path $repoRoot 'website/docs/public'
$iconPath = Join-Path $repoRoot 'editors/cfd-editor/src-tauri/icons'
$tauriCli = Join-Path $repoRoot 'editors/cfd-editor/node_modules/@tauri-apps/cli/tauri.js'

if (-not (Test-Path -LiteralPath $tauriCli)) {
    throw '请先运行 npm --prefix editors/cfd-editor ci，安装图标生成工具。'
}

# 网站与平台图标均派生自唯一 SVG；此命令只生成资源，不启动编辑器。
$logo = Get-Content -LiteralPath $logoPath -Raw
Copy-Item -LiteralPath $logoPath -Destination (Join-Path $publicPath 'logo.svg')
[IO.File]::WriteAllText((Join-Path $publicPath 'logo-dark.svg'), $logo.Replace('#3e63dd', '#9eb1ff'))
& node $tauriCli icon $logoPath --output $iconPath
if ($LASTEXITCODE -ne 0) { throw '平台图标生成失败。' }
Copy-Item -LiteralPath (Join-Path $iconPath '128x128@2x.png') -Destination (Join-Path $repoRoot 'editors/vscode-coflow/icon.png')
