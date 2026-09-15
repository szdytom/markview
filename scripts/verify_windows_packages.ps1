# Exercise both the portable executable and the executable stored in the MSI.
param([Parameter(Mandatory)][string]$OutDir)
$ErrorActionPreference = "Stop"
$out = (Resolve-Path $OutDir).Path
$work = Join-Path $env:RUNNER_TEMP ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $zip = Get-Item "$out/markview-x86_64-pc-windows-msvc.zip"
    Expand-Archive $zip.FullName "$work/portable"
    $portable = Get-ChildItem "$work/portable" -Recurse -Filter markview.exe
    if ($portable.Count -ne 1) { throw "expected one portable executable" }
    $process = Start-Process $portable.FullName -ArgumentList "--help" -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "portable executable failed: $($process.ExitCode)" }

    $msi = Get-Item "$out/markview-x86_64-pc-windows-msvc.msi"
    $log = "$work/msi.log"
    $process = Start-Process msiexec.exe -ArgumentList "/a `"$($msi.FullName)`" /qn TARGETDIR=`"$work/installed`" /L*v `"$log`"" -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        Get-Content $log
        throw "MSI extraction failed: $($process.ExitCode)"
    }
    $installed = Get-ChildItem "$work/installed" -Recurse -Filter markview.exe
    if ($installed.Count -ne 1) { throw "expected one MSI executable" }
    $process = Start-Process $installed.FullName -ArgumentList "--help" -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "MSI executable failed: $($process.ExitCode)" }
} finally {
    Remove-Item $work -Recurse -Force
}
