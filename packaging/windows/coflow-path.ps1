param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("Add", "Remove")]
    [string]$Action,

    [Parameter(Mandatory = $true)]
    [string]$Path
)

$ErrorActionPreference = "Stop"

function Normalize-PathEntry([string]$Value) {
    return $Value.Trim().Trim('"').TrimEnd('\').ToLowerInvariant()
}

$current = [Environment]::GetEnvironmentVariable("Path", "User")
$entries = @($current -split ';' | Where-Object { $_.Trim() -ne "" })
$normalizedTarget = Normalize-PathEntry $Path

if ($Action -eq "Add") {
    $contains = $entries | Where-Object { (Normalize-PathEntry $_) -eq $normalizedTarget }
    if (-not $contains) {
        $entries += $Path
    }
} else {
    $entries = @($entries | Where-Object { (Normalize-PathEntry $_) -ne $normalizedTarget })
}

[Environment]::SetEnvironmentVariable("Path", ($entries -join ';'), "User")
