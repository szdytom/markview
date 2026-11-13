# Exercise both the portable executable and the executable stored in the MSI.
param(
    [Parameter(Mandatory)][string]$OutDir,
    [ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")][string]$Target = "x86_64-pc-windows-msvc"
)
$ErrorActionPreference = "Stop"
$out = (Resolve-Path $OutDir).Path

function Remove-InstalledProduct {
    Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*" -ErrorAction SilentlyContinue |
        Where-Object { $_.DisplayName -eq "markview" } |
        ForEach-Object { Start-Process msiexec.exe -ArgumentList "/x $($_.PSChildName) /qn /norestart" -Wait | Out-Null }
}

$work = Join-Path $env:RUNNER_TEMP ([guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $zip = Get-Item "$out/markview-$Target.zip"
    Expand-Archive $zip.FullName "$work/portable"
    $portable = Get-ChildItem "$work/portable" -Recurse -Filter markview.exe
    if ($portable.Count -ne 1) { throw "expected one portable executable" }
    & "$PSScriptRoot/verify_windows_image.ps1" $portable.FullName $Target
    $process = Start-Process $portable.FullName -ArgumentList "--help" -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "portable executable failed: $($process.ExitCode)" }

    $msi = Get-Item "$out/markview-$Target.msi"
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $summary = $installer.SummaryInformation($msi.FullName, 0)
    $platform = if ($Target -eq "aarch64-pc-windows-msvc") { "Arm64" } else { "x64" }
    if ($summary.Property(7).Split(';')[0] -ne $platform) { throw "MSI platform is not $platform" }
    $log = Join-Path $work "msi.log"
    $installDir = Join-Path $work "installed"
    $process = Start-Process msiexec.exe -ArgumentList "/a `"$($msi.FullName)`" /qn TARGETDIR=`"$installDir`" /L*v `"$log`"" -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        Get-Content $log
        throw "MSI extraction failed: $($process.ExitCode)"
    }
    $installed = Get-ChildItem "$installDir" -Recurse -Filter markview.exe
    if ($installed.Count -ne 1) { throw "expected one MSI executable" }
    & "$PSScriptRoot/verify_windows_image.ps1" $installed.FullName $Target
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
