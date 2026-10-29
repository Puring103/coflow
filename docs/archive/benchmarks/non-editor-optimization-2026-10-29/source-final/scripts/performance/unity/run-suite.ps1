#requires -Version 7.0
param(
    [Parameter(Mandatory = $true)][string]$Player,
    [Parameter(Mandatory = $true)][ValidateSet('Mono', 'IL2CPP')][string]$Backend,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [switch]$Resume,
    [string[]]$Only,
    [switch]$List,
    [ValidateRange(1, 2147483)][int]$TimeoutSeconds = 1800
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$playerPath = (Resolve-Path -LiteralPath $Player).Path
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
$backendName = if ($Backend -eq 'Mono') { 'Mono' } else { 'IL2CPP' }
$prefix = 'unity-' + $backendName.ToLowerInvariant()
$runs = [Collections.Generic.List[object]]::new()

function Add-Run([string]$Name, [hashtable]$Overrides = @{}) {
    $parameters = [ordered]@{
        case = 'scalar20'; records = 10000; files = 100; samples = 5
        query_samples = 100; query_batch = 10000; variants = 1; density = 'sparse'
        topology = 'none'; templates = $false; operations = 'startup,load,io,reload,queries,error'
    }
    foreach ($key in $Overrides.Keys) { $parameters[$key] = $Overrides[$key] }
    $id = "$prefix-$Name"
    $runs.Add([pscustomobject][ordered]@{
        id = $id; dotnet_counterpart = "csharp-$Name"; backend = $backendName
        output = Join-Path $outputRoot "$id.stdout.ndjson"
        timeout_seconds = $TimeoutSeconds; parameters = [pscustomobject]$parameters
    })
}

# 与 .NET 矩阵保持夹具和操作一致，Unity 独有回退指标仍由玩家单独输出。
Add-Run 'scalar20-1k' @{ records = 1000; files = 10 }
Add-Run 'scalar20-10k'
Add-Run 'scalar20-100k' @{ records = 100000 }
Add-Run 'width8-10k' @{ case = 'scalar8'; operations = 'load,queries' }
Add-Run 'width100-10k' @{ case = 'scalar100'; operations = 'load,queries' }
Add-Run 'tables32-10k' @{ case = 'tables32'; operations = 'load,queries' }
Add-Run 'complex-10k' @{ case = 'complex'; topology = 'pairs' }
Add-Run 'complex-100k' @{ case = 'complex'; records = 100000; topology = 'pairs'; samples = 3; operations = 'load,reload,queries' }
Add-Run 'templates-10k' @{ case = 'complex'; topology = 'pairs'; templates = $true; operations = 'load,queries' }
Add-Run 'fanin-10k' @{ case = 'complex'; topology = 'hub'; templates = $true; operations = 'load,queries' }
Add-Run 'ring-10k' @{ case = 'complex'; topology = 'ring'; templates = $true; operations = 'load,queries,error' }
foreach ($variants in @(1, 10)) {
    foreach ($density in @('sparse', 'dense')) {
        Add-Run "dim$variants-$density-10k" @{
            case = 'dimensions'; variants = $variants; density = $density; topology = 'pairs'
            templates = $true; samples = 3; operations = 'load,dimensions,error'
        }
    }
}
Add-Run 'dim1-sparse-100k' @{
    case = 'dimensions'; records = 100000; topology = 'pairs'; templates = $true
    samples = 3; operations = 'load,dimensions'
}

$selected = $runs.ToArray()
if ($Only) {
    $requested = @($Only | ForEach-Object { $_.Split(',') } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    $unknown = @($requested | Where-Object { $_ -cnotin $runs.id })
    if ($unknown.Count -ne 0) { throw "Unknown run ids: $($unknown -join ', '). Use -List for the available ids." }
    $selected = @($runs | Where-Object { $_.id -cin $requested })
}
if ($selected.Count -eq 0) { throw 'No benchmark cases selected.' }
if ($List) { $selected.id; return }

function Compact-Json($Value) { ConvertTo-Json -InputObject $Value -Depth 12 -Compress }

function Get-PlayerArguments($Run) {
    $p = $Run.parameters
    @('-batchmode', '-nographics', '-logFile', "$($Run.output).player.log", '--output', $Run.output,
        '--case', $p.case, '--records', "$($p.records)", '--files', "$($p.files)", '--samples', "$($p.samples)",
        '--query-samples', "$($p.query_samples)", '--query-batch', "$($p.query_batch)", '--variants', "$($p.variants)",
        '--density', $p.density, '--topology', $p.topology, '--templates', $p.templates.ToString().ToLowerInvariant(), '--operations', $p.operations)
}

$playerRoot = [IO.Path]::GetDirectoryName($playerPath)
if ($outputRoot.Equals($playerRoot, [StringComparison]::OrdinalIgnoreCase) -or
    $outputRoot.StartsWith($playerRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Suite output must be outside the player build directory so recorded build payloads remain immutable.'
}
$buildMetadataPath = Join-Path $playerRoot 'build-metadata.json'
if (-not (Test-Path -LiteralPath $buildMetadataPath -PathType Leaf)) { throw "Missing isolated-player build metadata: $buildMetadataPath" }
$buildMetadata = Get-Content -LiteralPath $buildMetadataPath -Raw | ConvertFrom-Json
if ($buildMetadata.backend -cne $backendName -or $buildMetadata.result -cne 'Succeeded' -or $buildMetadata.development -ne $false) {
    throw "Player build metadata does not identify a successful non-Development $backendName build."
}
$playerHash = (Get-FileHash -LiteralPath $playerPath -Algorithm SHA256).Hash
$runnerPath = Join-Path $PSScriptRoot 'run-player.ps1'

# 玩家启动器本身不能标识实际程序集；记录整个构建载荷，避免改过 DLL 后误用旧结果。
$payload = @(Get-ChildItem -LiteralPath $playerRoot -File -Recurse | Where-Object { $_.Name -ne 'build.log' } | Sort-Object FullName | ForEach-Object {
    [pscustomobject][ordered]@{
        path = [IO.Path]::GetRelativePath($playerRoot, $_.FullName)
        bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash
    }
})
$plan = [ordered]@{
    schema_version = 1; backend = $backendName; player = $playerPath; player_sha256 = $playerHash
    runner_sha256 = (Get-FileHash -LiteralPath $runnerPath -Algorithm SHA256).Hash
    build_metadata = $buildMetadata; build_payload = $payload; runs = $runs.ToArray()
    protocol = 'Serial standalone players; resume requires matching plan, process arguments and successful semantic validation/completion. Failed evidence is never overwritten.'
}
$planPath = Join-Path $outputRoot "$prefix-plan.json"
if (Test-Path -LiteralPath $planPath) {
    $previousPlan = Get-Content -LiteralPath $planPath -Raw | ConvertFrom-Json
    if ((Compact-Json $previousPlan) -cne (Compact-Json $plan)) {
        throw "Existing suite plan does not match this player, payload or parameters: $planPath. Choose a new output directory; existing evidence is preserved."
    }
}
else {
    [void](New-Item -ItemType Directory -Path $outputRoot -Force)
    [IO.File]::WriteAllText($planPath, (ConvertTo-Json -InputObject $plan -Depth 12) + "`n", [Text.UTF8Encoding]::new($false))
}

function Assert-CompletedRun($Run) {
    $processPath = "$($Run.output).process.json"
    if (-not (Test-Path -LiteralPath $Run.output -PathType Leaf) -or -not (Test-Path -LiteralPath $processPath -PathType Leaf)) {
        throw "$($Run.id): existing evidence is incomplete. Choose a new output directory to retry; nothing was overwritten."
    }
    $process = Get-Content -LiteralPath $processPath -Raw | ConvertFrom-Json
    if ($process.exit_code -ne 0 -or $process.timed_out -ne $false -or $process.executable -cne $playerPath -or $process.executable_sha256 -cne $playerHash -or
        (Compact-Json @($process.arguments)) -cne (Compact-Json @(Get-PlayerArguments $Run))) {
        throw "$($Run.id): existing process result failed or its parameters/player differ. Existing evidence is preserved."
    }
    $rows = @(Get-Content -LiteralPath $Run.output | ForEach-Object { $_ | ConvertFrom-Json })
    $metadata = @($rows | Where-Object kind -eq 'metadata')
    $validation = @($rows | Where-Object kind -eq 'validation')
    if ($rows.Count -eq 0 -or $rows[-1].kind -cne 'complete' -or $rows[-1].success -ne $true -or
        @($rows | Where-Object kind -eq 'failure').Count -ne 0 -or $metadata.Count -ne 1 -or $validation.Count -ne 1 -or $validation[0].success -ne $true) {
        throw "$($Run.id): semantic validation/completion is missing or failed. Existing evidence is preserved."
    }
    $p = $Run.parameters
    $meta = $metadata[0]
    foreach ($name in @('case', 'records', 'files', 'variants', 'density', 'topology', 'templates')) {
        if ($meta.$name -cne $p.$name) { throw "$($Run.id): result metadata mismatch for $name. Existing evidence is preserved." }
    }
    if ($meta.backend -cne $backendName -or $meta.unity_version -cne $buildMetadata.unity_version -or $meta.development -ne $false -or
        (Compact-Json @($meta.operations)) -cne (Compact-Json @($p.operations.Split(',')))) {
        throw "$($Run.id): result host or operations do not match the plan. Existing evidence is preserved."
    }
}

# 先检查全部所选项，避免跑到半程才发现既有失败文件；每次玩家完成后再次验收。
$pending = [Collections.Generic.List[object]]::new()
foreach ($run in $selected) {
    $existing = @(@($run.output, "$($run.output).process.json", "$($run.output).player.log") | Where-Object { Test-Path -LiteralPath $_ })
    if ($existing.Count -gt 0) {
        if (-not $Resume) { throw "$($run.id): output evidence already exists. Use -Resume only for successful matching results, or choose a new output directory." }
        Assert-CompletedRun $run
        Write-Host "SKIP $($run.id) (validated successful result and matching parameters)"
    }
    else { $pending.Add($run) }
}
foreach ($run in $pending) {
    $p = $run.parameters
    Write-Host "RUN $($run.id)"
    & $runnerPath -Player $playerPath -Output $run.output -Case $p.case -Records $p.records -Files $p.files -Samples $p.samples `
        -QuerySamples $p.query_samples -QueryBatch $p.query_batch -Variants $p.variants -Density $p.density -Topology $p.topology `
        -Templates:$p.templates -Operations $p.operations -TimeoutSeconds $run.timeout_seconds
    Assert-CompletedRun $run
}
