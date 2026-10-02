# Exercise the submitted manifest with a real WinGet install.
param([string]$Manifest = "packaging/winget/0.1.10")
$ErrorActionPreference = "Stop"

winget validate --manifest $Manifest
if ($LASTEXITCODE -ne 0) { throw "WinGet manifest validation failed" }
winget settings --enable LocalManifestFiles
if ($LASTEXITCODE -ne 0) { throw "could not enable local WinGet manifests" }

try {
    winget install --manifest $Manifest --silent --disable-interactivity --accept-package-agreements --accept-source-agreements
    if ($LASTEXITCODE -ne 0) { throw "WinGet installation failed" }
    $product = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*" |
        Where-Object { $_.DisplayName -eq "markview" }
    if ($product.Count -ne 1) { throw "expected one installed Markview product" }
    if ($product.Publisher -ne "szdytom") { throw "unexpected MSI publisher" }
    $process = Start-Process "$env:ProgramFiles\markview\bin\markview.exe" -ArgumentList "--help" -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "installed executable failed: $($process.ExitCode)" }
} finally {
    Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*" |
        Where-Object { $_.DisplayName -eq "markview" } |
        ForEach-Object {
            $process = Start-Process msiexec.exe -ArgumentList "/x $($_.PSChildName) /qn /norestart" -Wait -PassThru
            if ($process.ExitCode -ne 0) { throw "MSI uninstall failed: $($process.ExitCode)" }
        }
}
