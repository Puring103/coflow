#requires -Version 7.0
param(
    [Parameter(Mandatory = $true)][string]$Unity,
    [Parameter(Mandatory = $true)][string]$ProjectPath,
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [ValidateSet('Mono', 'IL2CPP')][string]$Backend = 'Mono'
)
$ErrorActionPreference = 'Stop'
$project = (Resolve-Path -LiteralPath $ProjectPath).Path
if (-not (Test-Path -LiteralPath (Join-Path $project '.coflow-benchmark-project'))) { throw 'Only isolated benchmark projects can be built.' }
$output = [IO.Path]::GetFullPath($OutputPath)
if (Test-Path -LiteralPath $output) { throw "Build output must not exist: $output" }
[void](New-Item -ItemType Directory -Path $output)
$buildLog = Join-Path $output 'build.log'
$priorOutput = $env:COFLOW_BENCHMARK_OUTPUT
$priorBackend = $env:COFLOW_BENCHMARK_BACKEND
try {
    $env:COFLOW_BENCHMARK_OUTPUT = $output
    $env:COFLOW_BENCHMARK_BACKEND = $Backend
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = (Resolve-Path -LiteralPath $Unity).Path
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    foreach ($argument in @('-batchmode', '-nographics', '-quit', '-projectPath', $project, '-executeMethod', 'Coflow.Benchmarks.Editor.BuildBenchmark.Build', '-logFile', $buildLog)) {
        $startInfo.ArgumentList.Add($argument)
    }
    $process = [Diagnostics.Process]::Start($startInfo)
    $process.WaitForExit()
    $exitCode = $process.ExitCode
    $process.Dispose()
    if ($exitCode -ne 0) { throw "Unity $Backend build failed with exit code $exitCode. See $buildLog" }
    if (-not (Test-Path -LiteralPath (Join-Path $output 'CoflowBenchmark.exe'))) { throw "Unity produced no benchmark player. See $buildLog" }
    Get-Content -LiteralPath (Join-Path $output 'build-metadata.json')
}
finally {
    $env:COFLOW_BENCHMARK_OUTPUT = $priorOutput
    $env:COFLOW_BENCHMARK_BACKEND = $priorBackend
}
