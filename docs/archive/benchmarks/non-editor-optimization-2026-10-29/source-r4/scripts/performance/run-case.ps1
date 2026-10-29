param(
    [Parameter(Mandatory = $true)][string]$Spec,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [ValidateRange(1, 2147483647)][int]$TimeoutSeconds = 1800
)

$ErrorActionPreference = 'Stop'
$case = Get-Content -Raw -LiteralPath $Spec | ConvertFrom-Json
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$destination = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force $destination | Out-Null
$base = Join-Path $destination $case.id
# 可在两个独立进程之间暂停采样，编译和测试不会挤入任何正在计时的子进程。
$pauseRequest = Join-Path $destination 'pause-before.json'
if (Test-Path -LiteralPath $pauseRequest) {
    $pauseSpec = Get-Content -Raw -LiteralPath $pauseRequest | ConvertFrom-Json
    if ($pauseSpec.id -eq $case.id) {
        [ordered]@{id=$case.id;paused_at=(Get-Date).ToString('o');benchmark_started=$false} |
            ConvertTo-Json | Set-Content -LiteralPath "$base.paused.json" -Encoding utf8
        while (Test-Path -LiteralPath $pauseRequest) { Start-Sleep -Milliseconds 200 }
    }
}
# 计时前绑定实际执行文件与入口脚本；参数内的输出文件不作为输入摘要。
$executablePath = (Get-Command $case.executable -CommandType Application -ErrorAction Stop).Source
$inputHashes = @([ordered]@{path=$executablePath;sha256=(Get-FileHash -LiteralPath $executablePath -Algorithm SHA256).Hash})
foreach ($argument in $case.arguments) {
    if ([string]$argument -match '\.(dll|mjs)$' -and (Test-Path -LiteralPath $argument -PathType Leaf)) {
        $inputHashes += [ordered]@{path=[IO.Path]::GetFullPath($argument);sha256=(Get-FileHash -LiteralPath $argument -Algorithm SHA256).Hash}
    }
}
$startInfo = [Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $case.executable
$startInfo.WorkingDirectory = $repo
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$startInfo.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
foreach ($argument in $case.arguments) { $startInfo.ArgumentList.Add([string]$argument) }
if ($case.environment) {
    foreach ($property in $case.environment.PSObject.Properties) {
        $startInfo.Environment[$property.Name] = [string]$property.Value
    }
}

# 只启动当前基准子进程；计时和峰值包含夹具、预热、正确性对照，操作延迟由子进程另行输出。
$process = [Diagnostics.Process]::new()
$process.StartInfo = $startInfo
$timer = [Diagnostics.Stopwatch]::StartNew()
$startedAt = (Get-Date).ToString('o')
if (-not $process.Start()) { throw 'Failed to start benchmark' }
# 禁用托管写缓冲，让阶段标记在 CopyToAsync 收到后立即可读；异步复制不阻塞输出管道。
$outFile = [IO.FileStream]::new("$base.stdout.ndjson", [IO.FileMode]::Create, [IO.FileAccess]::Write, [IO.FileShare]::Read, 1, [IO.FileOptions]::Asynchronous)
$errFile = [IO.FileStream]::new("$base.stderr.log", [IO.FileMode]::Create, [IO.FileAccess]::Write, [IO.FileShare]::Read, 1, [IO.FileOptions]::Asynchronous)
$stdout = $process.StandardOutput.BaseStream.CopyToAsync($outFile)
$stderr = $process.StandardError.BaseStream.CopyToAsync($errFile)
$peak = [int64]0
$samples = 0
$memorySampleErrors = 0
$timedOut = $false
$sampleMemory = {
    # PeakWorkingSet64 是操作系统累计峰值；退出竞争只能记为缺测，不能把缺测解释成零内存。
    try {
        $process.Refresh()
        $observed = $process.PeakWorkingSet64
        if ($observed -gt 0) {
            $peak = [Math]::Max($peak, $observed)
            $samples++
        } else {
            $memorySampleErrors++
        }
    } catch [System.InvalidOperationException] {
        $memorySampleErrors++
    } catch [System.ComponentModel.Win32Exception] {
        $memorySampleErrors++
    }
}
. $sampleMemory
while (-not $process.WaitForExit(100)) {
    . $sampleMemory
    if ($timer.Elapsed.TotalSeconds -gt $TimeoutSeconds -and -not $process.HasExited) {
        $timedOut = $true
        try { $process.Kill($true) }
        catch [System.InvalidOperationException] { if (-not $process.HasExited) { throw } }
        $process.WaitForExit()
        break
    }
}
$timer.Stop()
# 尽力取得退出前最后一个区间的累计峰值；部分系统在退出后已无法读取进程内存。
. $sampleMemory
$null = $stdout.GetAwaiter().GetResult()
$null = $stderr.GetAwaiter().GetResult()
$outFile.Dispose()
$errFile.Dispose()
$exitCode = $process.ExitCode
$metadata = [ordered]@{
    id = $case.id
    spec = $case
    input_hashes = $inputHashes
    started_at = $startedAt
    process_id = $process.Id
    wall_seconds = $timer.Elapsed.TotalSeconds
    exit_code = $exitCode
    timed_out = $timedOut
    timeout_seconds = $TimeoutSeconds
    timeout_scope = $(if ($case.timeout_scope) { $case.timeout_scope } else { 'Whole benchmark process including fixture, warmup, timed samples and correctness validation; timeout is censored run evidence, not an operation latency.' })
    sampled_process_peak_working_set_bytes = $(if ($samples -gt 0) { $peak } else { $null })
    process_memory_samples = $samples
    process_memory_sample_errors = $memorySampleErrors
    memory_scope = $(if ($case.memory_scope) { $case.memory_scope } else { 'Top-level benchmark process, sampled every 100ms; includes fixture, warmup and oracle. Child CLI peaks require per-command metrics.' })
    memory_limitations = 'OS cumulative PeakWorkingSet64 is observed at startup, approximately every 100ms, and best-effort after exit. The last interval can be missed; no successful observations produce null, not zero. Descendant processes are excluded.'
}
$metadata | ConvertTo-Json -Depth 12 | Set-Content -Encoding utf8 "$base.meta.json"
Write-Output ($metadata | ConvertTo-Json -Compress -Depth 12)
$process.Dispose()
if ($timedOut) { exit 124 }
exit $exitCode
