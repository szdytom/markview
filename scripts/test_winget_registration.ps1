$ErrorActionPreference = "Stop"
$check = Join-Path $PSScriptRoot "check_winget_registration.ps1"
$output = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
$previousExitCode = $global:LASTEXITCODE

function gh {
    $global:LASTEXITCODE = $global:WingetTestExitCode
    $global:WingetTestResponse
}

try {
    foreach ($status in 200, 404, 403, 500) {
        $global:WingetTestExitCode = if ($status -eq 200) { 0 } else { 1 }
        $global:WingetTestResponse = "HTTP/2.0 $status`nContent-Type: application/json`n`n{}"
        $failed = $false
        try { & $check $output } catch { $failed = $true }
        if ($status -in 200, 404) {
            if ($failed) { throw "registration check failed for HTTP $status" }
            $expected = if ($status -eq 200) { "registered=true" } else { "registered=false" }
            if ((Get-Content $output -Raw).Trim() -ne $expected) { throw "unexpected output for HTTP $status" }
            Remove-Item $output
        } else {
            if (-not $failed) { throw "API failure was ignored for HTTP $status" }
            if (Test-Path $output) { throw "API failure produced registration output" }
        }
        Write-Output "PASS registration check: HTTP $status"
    }
} finally {
    Remove-Item $output -ErrorAction SilentlyContinue
    Remove-Variable WingetTestExitCode, WingetTestResponse -Scope Global
    $global:LASTEXITCODE = $previousExitCode
}
