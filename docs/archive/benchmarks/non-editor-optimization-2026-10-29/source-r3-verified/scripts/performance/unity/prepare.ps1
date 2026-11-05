param(
    [Parameter(Mandatory = $true)][string]$ProjectPath,
    [string]$LoaderDll,
    [string]$UnityVersion = '2022.3.62f3'
)
$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
$benchmark = Join-Path $repository 'runtimes/csharp/Coflow.Loader.Benchmarks'
$project = [IO.Path]::GetFullPath($ProjectPath)
if (Test-Path -LiteralPath $project) { throw "Isolated project must not exist: $project" }
if (-not $LoaderDll) { $LoaderDll = Join-Path $repository 'runtimes/csharp/Coflow.Loader/bin/Release/netstandard2.1/Coflow.Loader.dll' }
$loader = (Resolve-Path -LiteralPath $LoaderDll).Path
$utf8 = New-Object Text.UTF8Encoding($false)

# 仅创建新工程；夹具和生成类型直接来自 .NET 基准，转换不改变数据语义。
foreach ($directory in @('Assets/Scripts', 'Assets/Editor', 'Assets/Plugins', 'Assets/Generated', 'Packages', 'ProjectSettings')) {
    [void](New-Item -ItemType Directory -Path (Join-Path $project $directory) -Force)
}
foreach ($name in @('BenchmarkPlayer.cs', 'Adapter.cs', 'ConstructionContracts.cs')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $project "Assets/Scripts/$name")
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'BuildBenchmark.cs') -Destination (Join-Path $project 'Assets/Editor/BuildBenchmark.cs')
foreach ($name in @('Fixture.cs', 'Adapters.cs')) {
    $source = [IO.File]::ReadAllText((Join-Path $benchmark $name))
    if (-not $source.Contains('namespace Coflow.Benchmarks;')) { throw "Expected benchmark namespace: $name" }
    $source = "#nullable enable`nusing System;`nusing System.Collections.Generic;`nusing System.Linq;`n" + $source.Replace('namespace Coflow.Benchmarks;', 'namespace Coflow.Benchmarks {') + "`n}`n"
    [IO.File]::WriteAllText((Join-Path $project "Assets/Scripts/$name"), $source, $utf8)
}
foreach ($case in @('scalar8', 'scalar20', 'scalar100', 'tables32', 'complex', 'dimensions')) {
    Copy-Item -LiteralPath (Join-Path $benchmark "fixtures/$case/generated") -Destination (Join-Path $project "Assets/Generated/$case") -Recurse
}
Copy-Item -LiteralPath $loader -Destination (Join-Path $project 'Assets/Plugins/Coflow.Loader.dll')
[IO.File]::WriteAllText((Join-Path $project 'Packages/manifest.json'), '{"dependencies":{"com.unity.modules.jsonserialize":"1.0.0"}}', $utf8)
[IO.File]::WriteAllText((Join-Path $project 'ProjectSettings/ProjectVersion.txt'), "m_EditorVersion: $UnityVersion`n", $utf8)
[IO.File]::WriteAllText((Join-Path $project '.coflow-benchmark-project'), "Isolated Coflow benchmark project`n", $utf8)
$evidence = [ordered]@{
    project = $project; unity_version = $UnityVersion; loader = $loader
    loader_sha256 = (Get-FileHash -LiteralPath $loader -Algorithm SHA256).Hash
    fixture_sha256 = (Get-FileHash -LiteralPath (Join-Path $benchmark 'Fixture.cs') -Algorithm SHA256).Hash
    adapters_sha256 = (Get-FileHash -LiteralPath (Join-Path $benchmark 'Adapters.cs') -Algorithm SHA256).Hash
    contracts_sha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'ConstructionContracts.cs') -Algorithm SHA256).Hash
    utc = [DateTime]::UtcNow.ToString('o')
}
$evidence | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $project 'benchmark-inputs.json') -Encoding UTF8
$evidence | ConvertTo-Json -Compress
