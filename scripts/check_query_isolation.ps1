# PowerShell query isolation verification script
$ErrorActionPreference = "Stop"

Write-Host "Running query-path network isolation verification..."

$isolatedCrates = @("imgseek-core", "imgseek-features", "imgseek-index", "imgseek-store", "imgseek-query")
$denyNetRegex = '^(reqwest|hyper|ureq|isahc|surf|attohttpc|curl)$'

$failed = 0
foreach ($crate in $isolatedCrates) {
    $tree = cargo tree -p $crate --prefix none 2>$null
    $hasTokio = $false
    foreach ($line in ($tree -split "`n")) {
        $dep = ($line.Trim() -split '\s+')[0]
        if ([string]::IsNullOrWhiteSpace($dep)) { continue }
        if ($dep -eq "tokio") {
            $hasTokio = $true
        }
        if ($dep -match $denyNetRegex) {
            Write-Error "Query-path crate $crate depends on network crate: $dep"
            $failed = 1
        }
    }

    if ($hasTokio) {
        $tokioFeatures = cargo tree -p $crate -e features -i tokio 2>$null
        if ($tokioFeatures -match '\bnet\b') {
            Write-Error "Query-path crate $crate enables tokio net feature"
            $failed = 1
        }
    }
}

if ($failed -ne 0) {
    exit 1
}

Write-Host "Query isolation check passed: query crates have zero network dependencies."
