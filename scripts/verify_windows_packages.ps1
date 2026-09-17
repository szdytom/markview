# Exercise both the portable executable and the executable stored in the MSI.
param([Parameter(Mandatory)][string]$OutDir)
$ErrorActionPreference = "Stop"
$out = (Resolve-Path $OutDir).Path

# A shipped image must be a Windows-subsystem (GUI) executable, or opening the
# reader from Explorer puts a console window on screen. The optional header's
# Subsystem field sits 0x5C bytes past the PE signature.
function Get-PeSubsystem([string]$Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    if ([BitConverter]::ToUInt16($bytes, 0) -ne 0x5A4D) { throw "$Path is not a PE image" }
    $pe = [BitConverter]::ToInt32($bytes, 0x3C)
    if ([BitConverter]::ToUInt32($bytes, $pe) -ne 0x00004550) { throw "$Path has no PE header" }
    [BitConverter]::ToUInt16($bytes, $pe + 0x5C)
}

function Remove-InstalledProduct {
    Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*" -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -eq "markview" } |
        ForEach-Object { Start-Process msiexec.exe -ArgumentList "/x $($_.PSChildName) /qn /norestart" -Wait | Out-Null }
}

$work = Join-Path $env:RUNNER_TEMP ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $zip = Get-Item "$out/markview-x86_64-pc-windows-msvc.zip"
    Expand-Archive $zip.FullName "$work/portable"
    $portable = Get-ChildItem "$work/portable" -Recurse -Filter markview.exe
    if ($portable.Count -ne 1) { throw "expected one portable executable" }
    if ((Get-PeSubsystem $portable.FullName) -ne 2) { throw "portable executable is not a Windows-subsystem image" }
    $process = Start-Process $portable.FullName -ArgumentList "--help" -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "portable executable failed: $($process.ExitCode)" }

    $msi = Get-Item "$out/markview-x86_64-pc-windows-msvc.msi"
    $log = Join-Path $work "msi.log"
    $installDir = Join-Path $work "installed"
    $process = Start-Process msiexec.exe -ArgumentList "/a `"$($msi.FullName)`" /qn TARGETDIR=`"$installDir`" /L*v `"$log`"" -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        Get-Content $log
        throw "MSI extraction failed: $($process.ExitCode)"
    }
    $installed = Get-ChildItem "$installDir" -Recurse -Filter markview.exe
    if ($installed.Count -ne 1) { throw "expected one MSI executable" }
    if ((Get-PeSubsystem $installed.FullName) -ne 2) { throw "MSI executable is not a Windows-subsystem image" }
    $process = Start-Process $installed.FullName -ArgumentList "--help" -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "MSI executable failed: $($process.ExitCode)" }

    # An administrative install unpacks files and writes no registry, so the
    # Markdown association the MSI registers needs a real install. This is a
    # per-machine change: the runner is disposable, and the finally block puts
    # the machine back the way it was.
    try {
        Remove-InstalledProduct
        $process = Start-Process msiexec.exe -ArgumentList "/i `"$($msi.FullName)`" /qn /norestart" -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "MSI install failed: $($process.ExitCode)" }
        foreach ($extension in ".md", ".markdown", ".mdown") {
            $association = cmd /c "assoc $extension 2>&1"
            if ($association -notmatch "=Markview") { throw "MSI did not associate $extension`: $association" }
        }
        $registered = (Get-ItemProperty "HKLM:\SOFTWARE\RegisteredApplications" -ErrorAction SilentlyContinue).markview
        if ($registered -ne "SOFTWARE\markview\Capabilities") { throw "MSI did not register default-app capabilities: $registered" }
    } finally {
        Remove-InstalledProduct
    }
} finally {
    Remove-Item $work -Recurse -Force
}
