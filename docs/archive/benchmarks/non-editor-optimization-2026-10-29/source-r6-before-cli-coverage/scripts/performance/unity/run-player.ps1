#requires -Version 7.0
param(
    [Parameter(Mandatory = $true)][string]$Player,
    [Parameter(Mandatory = $true)][string]$Output,
    [ValidateSet('scalar8', 'scalar20', 'scalar100', 'tables32', 'complex', 'dimensions')][string]$Case = 'scalar20',
    [int]$Records = 1000,
    [int]$Files = 10,
    [int]$Samples = 5,
    [int]$QuerySamples = 100,
    [int]$QueryBatch = 10000,
    [int]$Variants = 1,
    [ValidateSet('sparse', 'dense')][string]$Density = 'sparse',
    [ValidateSet('none', 'pairs', 'hub', 'ring')][string]$Topology = 'none',
    [switch]$Templates,
    [string]$Operations = 'load,io,reload,queries,error',
    [int]$TimeoutSeconds = 1800
)
$ErrorActionPreference = 'Stop'
$destination = [IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $destination) { throw "Measurement output must not exist: $destination" }
[void](New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($destination)) -Force)
$startInfo = [Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = (Resolve-Path -LiteralPath $Player).Path
$startInfo.WorkingDirectory = [IO.Path]::GetDirectoryName($startInfo.FileName)
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$startInfo.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$arguments = @('-batchmode', '-nographics', '-logFile', "$destination.player.log", '--output', $destination,
    '--case', $Case, '--records', "$Records", '--files', "$Files", '--samples', "$Samples",
    '--query-samples', "$QuerySamples", '--query-batch', "$QueryBatch", '--variants', "$Variants",
    '--density', $Density, '--topology', $Topology, '--templates', $Templates.IsPresent.ToString().ToLowerInvariant(), '--operations', $Operations)
foreach ($argument in $arguments) { $startInfo.ArgumentList.Add($argument) }

# 每次独立玩家进程只运行一个夹具；墙钟与峰值额外包含 Unity 引擎启动和完整校验。
$timer = [Diagnostics.Stopwatch]::StartNew()
$started = [DateTime]::UtcNow.ToString('o')
$process = [Diagnostics.Process]::Start($startInfo)
$peak = [int64]0
$memorySamples = 0
$timedOut = $false
while (-not $process.WaitForExit(100)) {
    $process.Refresh()
    if (-not $process.HasExited) { $peak = [Math]::Max($peak, $process.PeakWorkingSet64); $memorySamples++ }
    if ($timer.Elapsed.TotalSeconds -gt $TimeoutSeconds) {
        $timedOut = $true
        $process.Kill($true)
        $process.WaitForExit()
        break
    }
}
$timer.Stop()
$exitCode = $process.ExitCode
$metadata = [ordered]@{
    executable = $startInfo.FileName; executable_sha256 = (Get-FileHash -LiteralPath $startInfo.FileName -Algorithm SHA256).Hash
    arguments = $arguments; started_at = $started; wall_seconds = $timer.Elapsed.TotalSeconds
    exit_code = $exitCode; timed_out = $timedOut; sampled_process_peak_working_set_bytes = $peak
    process_memory_samples = $memorySamples
    memory_scope = 'Entire Unity player process sampled every 100ms; includes engine, fixture, preflight, warmups, validation and all operations; separate from managed heap metrics.'
}
$metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$destination.process.json" -Encoding UTF8
$process.Dispose()
if ($timedOut) { throw "Unity benchmark timed out after $TimeoutSeconds seconds: $destination" }
if ($exitCode -ne 0) { throw "Unity benchmark failed with exit code $exitCode. See $destination.player.log" }
if (-not (Test-Path -LiteralPath $destination)) { throw "Unity player wrote no benchmark output. See $destination.player.log" }
$last = Get-Content -LiteralPath $destination -Tail 1 | ConvertFrom-Json
if ($last.kind -ne 'complete' -or $last.success -ne $true) { throw "Unity benchmark did not complete. See $destination" }
$metadata | ConvertTo-Json -Depth 8 -Compress
