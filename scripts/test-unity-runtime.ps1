param(
    [Parameter(Mandatory = $true)][string]$UnityEditor,
    [ValidateSet("Mono", "IL2CPP")][string]$Backend = "Mono"
)
$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent $PSScriptRoot
$project = Join-Path $repo "target/unity-runtime-smoke"
$player = Join-Path $repo "target/unity-runtime-$Backend/Smoke.exe"
$unity = (Resolve-Path -LiteralPath $UnityEditor).Path
# 独立工程只接收当前源码、绑定和原生库，所有 Unity 产物归入 target。
foreach ($directory in @("Assets/Runtime", "Assets/Generated", "Assets/Editor", "Assets/Resources", "Assets/Plugins/x86_64", "Packages", "ProjectSettings")) {
    New-Item -ItemType Directory -Force -Path (Join-Path $project $directory) | Out-Null
}
Copy-Item -Path "$repo/engine/runtimes/csharp/src/Coflow.Runtime/src/*.cs" -Destination "$project/Assets/Runtime" -Force
Copy-Item -Path "$repo/engine/runtimes/csharp/tests/integration/generated/*.cs" -Destination "$project/Assets/Generated" -Force
Copy-Item -LiteralPath "$repo/engine/runtimes/csharp/tests/integration/generated/coflow.contract" -Destination "$project/Assets/Resources/coflow.bytes" -Force
Copy-Item -LiteralPath "$repo/target/release/coflow_ffi.dll" -Destination "$project/Assets/Plugins/x86_64/coflow_ffi.dll" -Force
Copy-Item -LiteralPath "$repo/engine/runtimes/csharp/tests/unity/CoflowSmoke.cs" -Destination "$project/Assets/CoflowSmoke.cs" -Force
Copy-Item -LiteralPath "$repo/engine/runtimes/csharp/tests/unity/Editor/SmokeBuild.cs" -Destination "$project/Assets/Editor/SmokeBuild.cs" -Force
Set-Content -LiteralPath "$project/Packages/manifest.json" -Value '{"dependencies": {"com.unity.modules.jsonserialize": "1.0.0"}}' -Encoding utf8
$editorVersion = (Get-Item -LiteralPath $unity).Directory.Parent.Name
Set-Content -LiteralPath "$project/ProjectSettings/ProjectVersion.txt" -Value "m_EditorVersion: $editorVersion" -Encoding utf8
$env:COFLOW_UNITY_BACKEND = $Backend
$env:COFLOW_UNITY_OUTPUT = $player
$buildLog = Join-Path $repo "target/new-vm-unity-$Backend-build.log"
$buildProcess = Start-Process -FilePath $unity -ArgumentList @("-batchmode", "-nographics", "-quit", "-projectPath", ('"' + $project + '"'), "-executeMethod", "SmokeBuild.Build", "-logFile", ('"' + $buildLog + '"')) -WindowStyle Hidden -Wait -PassThru
if ($buildProcess.ExitCode -ne 0) { throw "Unity $Backend build failed. See $buildLog" }
$runLog = Join-Path $repo "target/new-vm-unity-$Backend-run.log"
$process = Start-Process -FilePath $player -ArgumentList @("-batchmode", "-nographics", "-logFile", ('"' + $runLog + '"')) -WindowStyle Hidden -Wait -PassThru
if ($process.ExitCode -ne 0 -or !(Select-String -LiteralPath $runLog -SimpleMatch 'coflow-unity-smoke-ok' -Quiet)) { throw "Unity $Backend smoke failed. See $runLog" }
Write-Output "Unity $Backend smoke passed."
