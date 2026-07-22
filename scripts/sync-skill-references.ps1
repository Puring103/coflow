param(
    [switch]$Check
)

$ErrorActionPreference = "Stop"

$RepoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$RepoPrefix = if ($RepoRoot.EndsWith([System.IO.Path]::DirectorySeparatorChar)) {
    $RepoRoot
} else {
    $RepoRoot + [System.IO.Path]::DirectorySeparatorChar
}
$PublicDocsBase = "https://puring103.github.io/coflow"
$Utf8NoBom = [System.Text.UTF8Encoding]::new($false)

$Mappings = @(
    @{
        Source = "website/docs/docs/reference/01-project-config.md"
        Target = "skills/coflow-workflow/references/project-config.md"
        Url = "$PublicDocsBase/docs/reference/01-project-config"
    }
    @{
        Source = "website/docs/docs/reference/02-project-pipeline.md"
        Target = "skills/coflow-workflow/references/project-pipeline.md"
        Url = "$PublicDocsBase/docs/reference/02-project-pipeline"
    }
    @{
        Source = "website/docs/docs/reference/05-data-model.md"
        Target = "skills/coflow-workflow/references/data-model.md"
        Url = "$PublicDocsBase/docs/reference/05-data-model"
    }
    @{
        Source = "website/docs/docs/reference/08-cli.md"
        Target = "skills/coflow-workflow/references/cli.md"
        Url = "$PublicDocsBase/docs/reference/08-cli"
    }
    @{
        Source = "website/docs/docs/reference/09-diagnostics/01-diagnostics.md"
        Target = "skills/coflow-workflow/references/diagnostics.md"
        Url = "$PublicDocsBase/docs/reference/09-diagnostics/01-diagnostics"
    }
    @{
        Source = "website/docs/docs/reference/03-language/01-cft.md"
        Target = "skills/coflow-schema/references/cft.md"
        Url = "$PublicDocsBase/docs/reference/03-language/01-cft"
    }
    @{
        Source = "website/docs/docs/reference/03-language/04-check.md"
        Target = "skills/coflow-schema/references/check.md"
        Url = "$PublicDocsBase/docs/reference/03-language/04-check"
    }
    @{
        Source = "website/docs/docs/reference/05-data-model.md"
        Target = "skills/coflow-schema/references/data-model.md"
        Url = "$PublicDocsBase/docs/reference/05-data-model"
    }
    @{
        Source = "website/docs/docs/reference/10-localization.md"
        Target = "skills/coflow-schema/references/localization.md"
        Url = "$PublicDocsBase/docs/reference/10-localization"
    }
    @{
        Source = "website/docs/docs/reference/11-schema-api.md"
        Target = "skills/coflow-schema/references/schema-api.md"
        Url = "$PublicDocsBase/docs/reference/11-schema-api"
    }
    @{
        Source = "website/docs/docs/reference/03-language/02-cfd.md"
        Target = "skills/coflow-data/references/cfd.md"
        Url = "$PublicDocsBase/docs/reference/03-language/02-cfd"
    }
    @{
        Source = "website/docs/docs/reference/03-language/03-cell-value.md"
        Target = "skills/coflow-data/references/cell-value.md"
        Url = "$PublicDocsBase/docs/reference/03-language/03-cell-value"
    }
    @{
        Source = "website/docs/docs/reference/04-sources/01-overview.md"
        Target = "skills/coflow-data/references/sources-overview.md"
        Url = "$PublicDocsBase/docs/reference/04-sources/01-overview"
    }
    @{
        Source = "website/docs/docs/reference/04-sources/02-table.md"
        Target = "skills/coflow-data/references/table-source.md"
        Url = "$PublicDocsBase/docs/reference/04-sources/02-table"
    }
    @{
        Source = "website/docs/docs/reference/04-sources/03-excel.md"
        Target = "skills/coflow-data/references/excel.md"
        Url = "$PublicDocsBase/docs/reference/04-sources/03-excel"
    }
    @{
        Source = "website/docs/docs/reference/04-sources/04-csv.md"
        Target = "skills/coflow-data/references/csv.md"
        Url = "$PublicDocsBase/docs/reference/04-sources/04-csv"
    }
    @{
        Source = "website/docs/docs/reference/08-cli.md"
        Target = "skills/coflow-data/references/cli.md"
        Url = "$PublicDocsBase/docs/reference/08-cli"
    }
)

function Get-RepoPath([string]$RelativePath) {
    $full = [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $RelativePath))
    if ($full -ne $RepoRoot -and -not $full.StartsWith($RepoPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Path escapes repository root: $RelativePath"
    }
    return $full
}

function Normalize-Lf([string]$Text) {
    return (($Text -replace "`r`n", "`n") -replace "`r", "`n")
}

function Get-ExpectedContent($Mapping) {
    $sourcePath = Get-RepoPath $Mapping.Source
    if (-not (Test-Path -LiteralPath $sourcePath)) {
        throw "Source document not found: $($Mapping.Source)"
    }

    $sourceText = Normalize-Lf ([System.IO.File]::ReadAllText($sourcePath))
    return $sourceText.TrimEnd() + "`n"
}

$outOfDate = [System.Collections.Generic.List[string]]::new()
$expectedTargets = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)

foreach ($mapping in $Mappings) {
    $targetPath = Get-RepoPath $mapping.Target
    [void]$expectedTargets.Add($targetPath)
    $expected = Get-ExpectedContent $mapping

    if (Test-Path -LiteralPath $targetPath) {
        $actual = Normalize-Lf ([System.IO.File]::ReadAllText($targetPath))
    } else {
        $actual = $null
    }

    if ($actual -ne $expected) {
        if ($Check) {
            $outOfDate.Add($mapping.Target)
            Write-Host "Out of date: $($mapping.Target) <- $($mapping.Source)"
        } else {
            $targetDir = Split-Path -Parent $targetPath
            New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
            [System.IO.File]::WriteAllText($targetPath, $expected, $Utf8NoBom)
            Write-Host "Synced: $($mapping.Target)"
        }
    }
}

$referenceDirs = $Mappings |
    ForEach-Object { Split-Path -Parent (Get-RepoPath $_.Target) } |
    Select-Object -Unique

foreach ($referenceDir in $referenceDirs) {
    if (-not (Test-Path -LiteralPath $referenceDir)) {
        continue
    }

    foreach ($file in Get-ChildItem -LiteralPath $referenceDir -File -Filter "generated-*.md") {
        if (-not $expectedTargets.Contains($file.FullName)) {
            $relative = [System.IO.Path]::GetRelativePath($RepoRoot, $file.FullName) -replace "\\", "/"
            if ($Check) {
                $outOfDate.Add($relative)
                Write-Host "Stale legacy reference: $relative"
            } else {
                Remove-Item -LiteralPath $file.FullName
                Write-Host "Removed stale legacy reference: $relative"
            }
        }
    }
}

if ($Check) {
    if ($outOfDate.Count -gt 0) {
        Write-Error "Skill synced references are out of date. Run: pwsh scripts/sync-skill-references.ps1"
        exit 1
    }

    Write-Host "Skill synced references are up to date."
}
