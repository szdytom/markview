# Distinguish a pending first submission from API failures.
param([Parameter(Mandatory)][string]$OutputFile)
$ErrorActionPreference = "Stop"

$response = gh api repos/microsoft/winget-pkgs/contents/manifests/s/szdytom/Markview --include 2>&1 | Out-String
if ($LASTEXITCODE -eq 0) {
    "registered=true" >> $OutputFile
} elseif ($response -match '(?m)^HTTP/\S+ 404\b') {
    "registered=false" >> $OutputFile
    Write-Host "::notice::The initial WinGet submission is pending; skipping the update. After it merges, run this workflow for the latest stable release."
} else {
    throw "Could not check WinGet registration: $response"
}
