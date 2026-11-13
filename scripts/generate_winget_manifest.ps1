# Inspect both MSI packages; every build has its own product codes and hashes.
param(
    [Parameter(Mandatory)][string]$MsiDir,
    [Parameter(Mandatory)][string]$BaseUrl,
    [Parameter(Mandatory)][string]$ReleaseUrl,
    [Parameter(Mandatory)][string]$Version,
    [string]$OutDir = "."
)
$ErrorActionPreference = "Stop"
$installer = New-Object -ComObject WindowsInstaller.Installer
$metadata = foreach ($entry in @(
    @{ Architecture = "x64"; Target = "x86_64-pc-windows-msvc"; Platform = "x64" },
    @{ Architecture = "arm64"; Target = "aarch64-pc-windows-msvc"; Platform = "Arm64" }
)) {
    $name = "markview-$($entry.Target).msi"
    $msi = Get-Item (Join-Path $MsiDir $name)
    $summary = $installer.SummaryInformation($msi.FullName, 0)
    if ($summary.Property(7).Split(';')[0] -ne $entry.Platform) { throw "unexpected MSI platform: $name" }
    $database = $installer.OpenDatabase($msi.FullName, 0)
    $properties = @{}
    foreach ($property in "ProductVersion", "ProductCode", "UpgradeCode", "Manufacturer", "ProductName") {
        $view = $database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property`` = '$property'")
        [void]$view.Execute()
        $properties[$property] = $view.Fetch().StringData(1)
        [void]$view.Close()
    }
    @{
        Architecture = $entry.Architecture
        InstallerUrl = "$($BaseUrl.TrimEnd('/'))/$name"
        InstallerSha256 = (Get-FileHash $msi.FullName -Algorithm SHA256).Hash
        ProductVersion = $properties.ProductVersion
        ProductCode = $properties.ProductCode
        UpgradeCode = $properties.UpgradeCode
        Publisher = $properties.Manufacturer
        DisplayName = $properties.ProductName
    }
}
$path = Join-Path ([IO.Path]::GetTempPath()) "$([guid]::NewGuid()).json"
try {
    ConvertTo-Json -InputObject @($metadata) | Set-Content $path -Encoding utf8
    python "$PSScriptRoot/generate_winget_manifest.py" $path --version $Version --release-url $ReleaseUrl --out $OutDir
    if ($LASTEXITCODE -ne 0) { throw "WinGet manifest generation failed" }
} finally {
    Remove-Item $path -Force
}
