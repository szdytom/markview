# Exercise installation checks and failure cleanup without downloading Scoop.
$ErrorActionPreference = 'Stop'
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ("markview scoop-" + [guid]::NewGuid())
$previousEnvironment = @{}
foreach ($name in 'TEMP', 'SCOOP', 'SCOOP_GLOBAL', 'SCOOP_CACHE', 'XDG_CONFIG_HOME') {
    $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
}
$previousExitCode = $global:LASTEXITCODE
$state = @{}
function Get-Command { [pscustomobject]@{ Source = "$fixtureRoot/source/shims/scoop.ps1" } }
function Join-Path {
    param([string]$Path, [string]$ChildPath)
    if ($ChildPath -eq 'Programs/Scoop Apps') { $Path = "$fixtureRoot/start-menu" }
    Microsoft.PowerShell.Management\Join-Path $Path $ChildPath
}
function New-Object {
    param([string]$ComObject)
    if ($ComObject -ne 'WScript.Shell') { throw 'unexpected COM object' }
    if ($IsWindows) { return Microsoft.PowerShell.Utility\New-Object -ComObject $ComObject }
    $shell = [pscustomobject]@{}
    $shell | Add-Member ScriptMethod CreateShortcut {
        param($Path)
        $link = [pscustomobject]@{ Target = Get-Content $Path -Raw }
        $link | Add-Member ScriptProperty TargetPath { $this.Target } {
            # Model the shell's normalization of path separators.
            $this.Target = $args[0].Replace('/', '\')
        }
        return $link
    }
    return $shell
}
function Start-Process {
    param($FilePath, $ArgumentList, [string]$WindowStyle, [switch]$Wait, [switch]$PassThru)
    $state.Launched = $true
    [pscustomobject]@{ ExitCode = 0 }
}
try {
    $env:TEMP = Join-Path $fixtureRoot 'temp'
    $source = "$fixtureRoot/source/apps/scoop/current/bin"
    New-Item -ItemType Directory $source, $env:TEMP -Force | Out-Null
    @'
$state.ScoopRoot = $env:SCOOP
if ($args[0] -eq 'install') {
    $version = Join-Path $env:SCOOP 'apps/markview/0.5.2'
    $current = Join-Path $env:SCOOP 'apps/markview/current'
    New-Item -ItemType Directory $version -Force | Out-Null
    $linkType = if ($IsWindows) { 'Junction' } else { 'SymbolicLink' }
    New-Item -ItemType $linkType $current -Target $version | Out-Null
    if ($IsWindows) { attrib.exe +R /L $current }
    # The Windows shell requires a valid PE image when saving a shortcut.
    $bytes = if ($IsWindows) { [IO.File]::ReadAllBytes((Join-Path $PSHOME 'pwsh.exe')) } else { [byte[]]::new(256) }
    $pe = if ($IsWindows) { [BitConverter]::ToInt32($bytes, 0x3C) } else { 0x80 }
    [BitConverter]::GetBytes([uint16]0x5A4D).CopyTo($bytes, 0)
    [BitConverter]::GetBytes([int32]$pe).CopyTo($bytes, 0x3C)
    [BitConverter]::GetBytes([uint32]0x00004550).CopyTo($bytes, $pe)
    [BitConverter]::GetBytes([uint16]0x8664).CopyTo($bytes, $pe + 4)
    [BitConverter]::GetBytes([uint16]0x20B).CopyTo($bytes, $pe + 24)
    [BitConverter]::GetBytes([uint16]2).CopyTo($bytes, $pe + 92)
    $installed = Join-Path $current 'markview.exe'
    [IO.File]::WriteAllBytes($installed, $bytes)
    New-Item -ItemType File (Join-Path $env:SCOOP 'shims/markview.exe') | Out-Null
    $shortcut = [IO.Path]::GetFullPath((Join-Path "$fixtureRoot/start-menu" 'Programs/Scoop Apps/Markview.lnk'))
    New-Item -ItemType Directory (Split-Path $shortcut) -Force | Out-Null
    $target = if ($state.WrongTarget) { Join-Path $current 'wrong.exe' } else { $installed }
    if ($IsWindows) {
        $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
        $link.TargetPath = [IO.Path]::GetFullPath($target)
        $link.Save()
    } else {
        [IO.File]::WriteAllText($shortcut, $target.Replace('/', '\'))
    }
} elseif ($args[0] -eq 'uninstall') {
    $state.Uninstalled = $true
    $current = Get-Item (Join-Path $env:SCOOP 'apps/markview/current')
    $current.Attributes = $current.Attributes -band (-bnot [IO.FileAttributes]::ReadOnly)
    [IO.Directory]::Delete($current.FullName)
    Remove-Item (Join-Path $env:SCOOP 'apps/markview') -Recurse -Force
    Remove-Item (Join-Path $env:SCOOP 'shims/markview.exe')
    Remove-Item (Join-Path "$fixtureRoot/start-menu" 'Programs/Scoop Apps/Markview.lnk')
} else { throw 'unexpected Scoop command' }
exit 0
'@ | Set-Content "$source/scoop.ps1"
    foreach ($scenario in 'success', 'wrong-target', 'wrong-architecture') {
        $state.Clear()
        $state.WrongTarget = $scenario -eq 'wrong-target'
        $architecture = if ($scenario -eq 'wrong-architecture') { 'arm64' } else { '64bit' }
        $failure = $null
        try { & "$PSScriptRoot/verify_scoop_package.ps1" -Architecture $architecture } catch { $failure = $_ }
        if ($scenario -eq 'success') {
            if ($failure) { throw $failure }
            if (!$state.Launched -or !$state.Uninstalled) { throw 'installation was not verified and uninstalled' }
        } else {
            $expected = if ($scenario -eq 'wrong-target') { 'Unexpected shortcut target:*' } else { '*is not a aarch64-pc-windows-msvc image' }
            if (!$failure -or $failure.Exception.Message -notlike $expected) { throw "incorrect failure for ${scenario}: $failure" }
            if ($state.Launched -or $state.Uninstalled) { throw 'invalid installation was accepted' }
        }
        if (Test-Path (Split-Path $state.ScoopRoot)) { throw 'temporary installation was not removed' }
        if (Test-Path "$fixtureRoot/start-menu/Programs/Scoop Apps/Markview.lnk") { throw 'shortcut was not removed' }
    }
    Write-Output 'PASS Scoop target normalization, installation checks and junction cleanup'
} finally {
    foreach ($name in $previousEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name])
    }
    $global:LASTEXITCODE = $previousExitCode
    Remove-Item $fixtureRoot -Recurse -Force
}
