param([string]$Coflow = 'coflow')
$ErrorActionPreference = 'Stop'
# 每个场景使用独立的真实生成类型，表数量不会被其他场景的注册项污染。
foreach ($case in @('scalar8', 'scalar20', 'scalar100', 'tables32', 'complex', 'dimensions')) {
    & $Coflow codegen --project (Join-Path $PSScriptRoot "fixtures/$case")
    if ($LASTEXITCODE -ne 0) { throw "Code generation failed for $case" }
}
