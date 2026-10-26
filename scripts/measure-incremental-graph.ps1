param(
    [Parameter(Mandatory)][string]$BaselineExe,
    [string]$CurrentExe = "$PSScriptRoot/../target/release/examples/engine_perf.exe",
    [string]$OutputDirectory = "$PSScriptRoot/../docs/archive/benchmarks",
    [switch]$ColdOnly,
    [switch]$ScalingOnly
)
$ErrorActionPreference = 'Stop'
$metadataName = if ($ColdOnly) { 'incremental-graph-engine-cold-2026-10-26.json' } elseif ($ScalingOnly) { 'incremental-graph-engine-scaling-2026-10-26.json' } else { 'incremental-graph-engine-acceptance-2026-10-26.json' }
$metadata = [ordered]@{
    date = (Get-Date).ToUniversalTime().ToString('o')
    ref = (git rev-parse HEAD)
    baseline_sha256 = (Get-FileHash -LiteralPath $BaselineExe).Hash
    current_sha256 = (Get-FileHash -LiteralPath $CurrentExe).Hash
    rust = (rustc -vV) -join "`n"
    protocol = 'Serial Release processes, deterministic 100-file/20-field fixture. Raw mutation sample count 219: example warmup 1 plus first 19 discarded gives 20 warmups and 200 retained samples. Ten fresh-session cold samples in round 0; later rounds 1. Working-set monitor at 100ms; cold process peak captured at data_open_stats, includes schema and all fresh-session opens. Additional batch run has only 3 samples, not stable P95.'
    runs = [Collections.Generic.List[object]]::new()
}

function Measure-Run([string]$Label, [string]$Executable, [int]$Round, [int]$Records, [string]$Batches, [int]$Samples, [int]$Updates, [int]$Files = 100) {
    $name = "incremental-graph-$Label-$Records-$Batches-r$Round-2026-10-26"
    Write-Output "start $name"
    $info = [Diagnostics.ProcessStartInfo]::new([IO.Path]::GetFullPath($Executable))
    $info.UseShellExecute = $false
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.CreateNoWindow = $true
    $info.Environment['ENGINE_MODE'] = 'pipeline'
    $info.Environment['ENGINE_RECORDS'] = "$Records"
    $info.Environment['ENGINE_FILES'] = "$Files"
    $info.Environment['ENGINE_SAMPLES'] = "$Samples"
    $info.Environment['ENGINE_UPDATE_SAMPLES'] = "$Updates"
    $info.Environment['ENGINE_VALIDATION_SAMPLES'] = '3'
    $info.Environment['ENGINE_BATCHES'] = $Batches
    $process = [Diagnostics.Process]::Start($info)
    $stderr = $process.StandardError.ReadToEndAsync()
    $line = $process.StandardOutput.ReadLineAsync()
    $lines = [Collections.Generic.List[string]]::new()
    $peak = 0L
    $coldPeak = 0L
    while ($null -ne $line) {
        if ($line.IsCompleted) {
            $value = $line.GetAwaiter().GetResult()
            if ($null -eq $value) { break }
            $lines.Add($value)
            if ($value.Contains('"metric":"data_open_stats"')) { $coldPeak = $peak }
            $line = $process.StandardOutput.ReadLineAsync()
        } else {
            if (!$process.HasExited) {
                $process.Refresh()
                $peak = [Math]::Max($peak, $process.PeakWorkingSet64)
            }
            Start-Sleep -Milliseconds 100
        }
    }
    $process.WaitForExit()
    $errorText = $stderr.GetAwaiter().GetResult()
    [IO.File]::WriteAllLines((Join-Path $OutputDirectory "$name.ndjson"), $lines)
    if ($process.ExitCode -ne 0) { throw "$name failed ($($process.ExitCode)): $errorText" }
    $metadata.runs.Add([ordered]@{ label=$Label; round=$Round; records=$Records; files=$Files; batches=$Batches; raw="$name.ndjson"; cold_peak_working_set_bytes=$coldPeak; peak_working_set_bytes=$peak; stderr=$errorText })
    [IO.File]::WriteAllText((Join-Path $OutputDirectory $metadataName), ($metadata | ConvertTo-Json -Depth 8))
    $process.Dispose()
    Write-Output "done $name"
}

if ($ScalingOnly) {
    Measure-Run baseline $BaselineExe 4 10000 '1' 1 219 10
    Measure-Run current $CurrentExe 4 10000 '1' 1 219 10
    exit
}
if ($ColdOnly) {
    # 反转执行顺序复核冷加载波动，全部原始样本保留，不按结果剔除慢样本。
    Measure-Run current $CurrentExe 3 100000 '1' 10 3
    Measure-Run baseline $BaselineExe 3 100000 '1' 10 3
    exit
}
for ($round = 0; $round -lt 3; $round++) {
    $samples = if ($round -eq 0) { 10 } else { 1 }
    Measure-Run baseline $BaselineExe $round 100000 '1' $samples 219
    Measure-Run current $CurrentExe $round 100000 '1' $samples 219
}
Measure-Run baseline $BaselineExe 0 10000 '1' 1 219
Measure-Run current $CurrentExe 0 10000 '1' 1 219
Measure-Run baseline $BaselineExe 0 100000 '1000' 1 3
Measure-Run current $CurrentExe 0 100000 '1000' 1 3
