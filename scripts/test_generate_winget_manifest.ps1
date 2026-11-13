# Exercise MSI inspection with distinct per-architecture properties and files.
$ErrorActionPreference = "Stop"
class MsiRecord {
    [string]$Value
    MsiRecord([string]$value) { $this.Value = $value }
    [string] StringData([int]$column) { return $this.Value }
}
class MsiView {
    [string]$Value
    MsiView([string]$value) { $this.Value = $value }
    # COM calls can emit values into the surrounding metadata collection.
    [object] Execute() { return $null }
    [MsiRecord] Fetch() { return [MsiRecord]::new($this.Value) }
    [object] Close() { return $null }
}
class MsiDatabase {
    [bool]$Arm
    MsiDatabase([bool]$arm) { $this.Arm = $arm }
    [MsiView] OpenView([string]$query) {
        $values = @{
            ProductVersion = '0.6.0'; ProductName = 'markview'; Manufacturer = 'szdytom'
            ProductCode = $(if ($this.Arm) { '{22222222-2222-2222-2222-222222222222}' } else { '{11111111-1111-1111-1111-111111111111}' })
            UpgradeCode = '{1094AD43-1728-41AA-A880-31382AA58F5B}'
        }
        if ($query -notmatch "^SELECT ``Value`` FROM ``Property`` WHERE ``Property`` = '([^']+)'$") { throw 'unexpected MSI query' }
        return [MsiView]::new($values[$Matches[1]])
    }
}
class MsiSummary {
    [string]$Platform
    MsiSummary([string]$platform) { $this.Platform = $platform }
    [string] Property([int]$index) { return "$($this.Platform);1033" }
}
class MsiInstaller {
    static [bool]$WrongPlatform = $false
    [MsiSummary] SummaryInformation([string]$path, [int]$mode) {
        $platform = if ($path.Contains('aarch64') -and -not [MsiInstaller]::WrongPlatform) { 'Arm64' } else { 'x64' }
        return [MsiSummary]::new($platform)
    }
    [MsiDatabase] OpenDatabase([string]$path, [int]$mode) { return [MsiDatabase]::new($path.Contains('aarch64')) }
}
function New-Object {
    param([string]$ComObject)
    if ($ComObject -ne 'WindowsInstaller.Installer') { throw 'unexpected COM object' }
    return [MsiInstaller]::new()
}
$work = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
New-Item -ItemType Directory $work | Out-Null
try {
    foreach ($target in 'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc') {
        Set-Content (Join-Path $work "markview-$target.msi") $target
    }
    $arguments = @{
        MsiDir = $work; BaseUrl = 'https://example.com/v0.6.0'; ReleaseUrl = 'https://example.com/notes'
        Version = '0.6.0'; OutDir = $work
    }
    & "$PSScriptRoot/generate_winget_manifest.ps1" @arguments | Out-Null
    $path = Join-Path $work 'manifests/s/szdytom/Markview/0.6.0/szdytom.Markview.installer.yaml'
    $manifest = python -c 'import json,sys,yaml; print(json.dumps(yaml.safe_load(open(sys.argv[1]))))' $path | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'could not read generated manifest' }
    $index = 0
    foreach ($target in 'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc') {
        $entry = $manifest.Installers[$index++]
        $name = "markview-$target.msi"
        if ($entry.InstallerUrl -ne "https://example.com/v0.6.0/$name") { throw 'incorrect installer URL' }
        if ($entry.InstallerSha256 -ne (Get-FileHash (Join-Path $work $name)).Hash) { throw 'incorrect MSI hash' }
        $code = if ($target.StartsWith('aarch64')) { '{22222222-2222-2222-2222-222222222222}' } else { '{11111111-1111-1111-1111-111111111111}' }
        if ($entry.ProductCode -ne $code) { throw 'incorrect MSI product code' }
    }
    foreach ($failure in 'platform', 'version', 'missing') {
        [MsiInstaller]::WrongPlatform = $failure -eq 'platform'
        $arguments.Version = if ($failure -eq 'version') { '0.6.1' } else { '0.6.0' }
        if ($failure -eq 'missing') { Remove-Item (Join-Path $work 'markview-aarch64-pc-windows-msvc.msi') }
        $rejected = $false
        try { & "$PSScriptRoot/generate_winget_manifest.ps1" @arguments 2>$null | Out-Null } catch { $rejected = $true }
        if (-not $rejected) { throw "accepted incorrect MSI $failure" }
    }
    Write-Output 'PASS dual-architecture MSI inspection, metadata and failure handling'
} finally {
    Remove-Item $work -Recurse -Force
    $global:LASTEXITCODE = 0
}
