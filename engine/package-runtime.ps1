param(
    [switch]$Check,
    [switch]$CftCompiler,
    [string]$Target,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$engineRoot = $PSScriptRoot
$repositoryRoot = Split-Path $engineRoot
$temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$stage = Join-Path $temporaryRoot ('coflow-engine-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null

try {
    $workspace = Join-Path $stage 'source'
    New-Item -ItemType Directory -Path $workspace | Out-Null
    $members = @()
    foreach ($crate in Get-ChildItem -LiteralPath (Join-Path $engineRoot 'crates') -Directory) {
        $destination = Join-Path $workspace "crates/$($crate.Name)"
        New-Item -ItemType Directory -Path $destination | Out-Null
        $manifest = Get-Content -LiteralPath (Join-Path $crate.FullName 'Cargo.toml') -Raw
        # 生产构建只携带库源码；测试依赖与 benchmark target 不进入独立构建包。
        $manifest = [regex]::Replace($manifest, '(?ms)^\[dev-dependencies\]\r?\n.*?(?=^\[|\z)', '')
        $manifest = [regex]::Replace($manifest, '(?ms)^\[\[bench\]\]\r?\n.*?(?=^\[|\z)', '')
        [IO.File]::WriteAllText((Join-Path $destination 'Cargo.toml'), $manifest)
        Copy-Item -LiteralPath (Join-Path $crate.FullName 'src') -Destination $destination -Recurse
        $members += '"crates/' + $crate.Name + '"'
    }
    # 独立 workspace 没有 Studio 成员，CFT 编译与 ts-export 不会被其 feature 合并启用。
    $rootManifest = Get-Content -LiteralPath (Join-Path $repositoryRoot 'Cargo.toml') -Raw
    $settings = [regex]::Matches($rootManifest, '(?ms)^\[(?:workspace\.lints\.[^\]]+|profile\.[^\]]+)\]\r?\n.*?(?=^\[|\z)')
    $manifest = "[workspace]`nresolver = `"2`"`nmembers = [" + ($members -join ', ') + "]`n"
    $manifest += "default-members = [`"crates/coflow-ffi`"]`n`n" + (($settings | ForEach-Object Value) -join "`n")
    [IO.File]::WriteAllText((Join-Path $workspace 'Cargo.toml'), $manifest)
    Copy-Item -LiteralPath (Join-Path $repositoryRoot 'Cargo.lock') -Destination $workspace

    Push-Location $workspace
    try {
        $metadataText = & cargo metadata --format-version 1 --no-deps
        if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve isolated Engine workspace' }
        $metadata = $metadataText | ConvertFrom-Json
        foreach ($package in $metadata.packages) {
            foreach ($dependency in $package.dependencies) {
                if ($dependency.path -and -not ([IO.Path]::GetFullPath($dependency.path)).StartsWith($workspace + [IO.Path]::DirectorySeparatorChar)) {
                    throw "Engine dependency escapes its source package: $($dependency.path)"
                }
            }
        }
        $arguments = @('-p', 'coflow-ffi', '--lib', '--target-dir', (Join-Path $repositoryRoot 'target/engine-runtime'))
        if ($CftCompiler) { $arguments += @('--features', 'cft-compiler') }
        if ($Target) { $arguments += @('--target', $Target) }
        if ($Check) {
            & cargo check @arguments
            if ($LASTEXITCODE -ne 0) { throw 'Isolated Engine check failed' }
            Write-Host 'Engine builds from its isolated production sources without Studio.'
            return
        }
        $buildOutput = & cargo build @arguments --release --message-format=json
        if ($LASTEXITCODE -ne 0) { throw 'Isolated Engine build failed' }
        $artifacts = @($buildOutput | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object {
            $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'coflow_ffi'
        })
    } finally {
        Pop-Location
    }

    $payload = Join-Path $stage 'runtime'
    $native = Join-Path $payload 'native'
    $csharp = Join-Path $payload 'csharp'
    New-Item -ItemType Directory -Path $native, $csharp | Out-Null
    $libraries = @($artifacts.filenames | Where-Object { [IO.Path]::GetExtension($_) -in @('.dll', '.so', '.dylib', '.lib', '.a') })
    if ($libraries.Count -eq 0) { throw 'Cargo produced no native runtime libraries' }
    foreach ($library in $libraries) { Copy-Item -LiteralPath $library -Destination $native }
    Copy-Item -LiteralPath (Join-Path $engineRoot 'crates/coflow-ffi/include/coflow.h') -Destination $native
    Copy-Item -LiteralPath (Join-Path $repositoryRoot 'LICENSE') -Destination $payload
    $wrapper = Join-Path $engineRoot 'runtimes/csharp/src/Coflow.Runtime'
    # 交付内容使用白名单，排除 bin/obj、测试、基准和工具项目。
    foreach ($entry in @('src', 'Coflow.Runtime.csproj', 'package.json', 'README.md')) {
        Copy-Item -LiteralPath (Join-Path $wrapper $entry) -Destination $csharp -Recurse
    }
    $version = ($metadata.packages | Where-Object name -EQ 'coflow-ffi').version
    if (-not $Target) {
        if ($env:CARGO_BUILD_TARGET) { $Target = $env:CARGO_BUILD_TARGET }
        else {
            $rustInfo = & rustc -vV
            if ($LASTEXITCODE -ne 0) { throw 'Cannot identify Rust host target' }
            $Target = ($rustInfo | Select-String '^host: (.+)$').Matches[0].Groups[1].Value
        }
    }
    if (-not $OutputDirectory) { $OutputDirectory = Join-Path $repositoryRoot 'dist/engine' }
    New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
    $variant = if ($CftCompiler) { '-compiler' } else { '' }
    $archive = Join-Path $OutputDirectory "coflow-runtime-$version-$Target$variant.zip"
    if (Test-Path -LiteralPath $archive) { throw "Runtime archive already exists: $archive" }
    Compress-Archive -LiteralPath $native, $csharp, (Join-Path $payload 'LICENSE') -DestinationPath $archive
    Write-Host "Runtime package: $archive"
} finally {
    # 仅清理本次创建并校验过的临时目录，不触碰仓库或用户指定的输出目录。
    $resolvedStage = [IO.Path]::GetFullPath($stage)
    if ($resolvedStage.StartsWith($temporaryRoot) -and (Split-Path $resolvedStage -Leaf) -match '^coflow-engine-[0-9a-f]{32}$') {
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force
    }
}
