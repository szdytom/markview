# Run in a fresh PowerShell process with Scoop already installed.
param(
    [string]$Manifest = "$PSScriptRoot/../packaging/scoop/markview.json",
    [ValidateSet('64bit', 'arm64')][string]$Architecture = '64bit'
)
$ErrorActionPreference = 'Stop'
$manifestPath = (Resolve-Path $Manifest).Path
$scoopCommand = (Get-Command scoop.ps1).Source
$scoopSource = Join-Path (Split-Path (Split-Path $scoopCommand)) 'apps/scoop/current'
$work = Join-Path $env:TEMP ("markview-scoop-" + [guid]::NewGuid())
$shortcutDirectory = Join-Path ([Environment]::GetFolderPath('StartMenu')) 'Programs/Scoop Apps'
$shortcut = [IO.Path]::GetFullPath((Join-Path $shortcutDirectory 'Markview.lnk'))
if (Test-Path $shortcut) { throw "Refusing to replace existing shortcut: $shortcut" }
$hadShortcutDirectory = Test-Path $shortcutDirectory
try {
    $env:SCOOP = Join-Path $work 'scoop'
    $env:SCOOP_GLOBAL = Join-Path $work 'global'
    $env:SCOOP_CACHE = Join-Path $work 'cache'
    $env:XDG_CONFIG_HOME = Join-Path $work 'config'
    $scoopCopy = Join-Path $env:SCOOP 'apps/scoop/current'
    New-Item -ItemType Directory -Path $scoopCopy -Force | Out-Null
    New-Item -ItemType Directory -Path "$env:SCOOP/shims", "$env:SCOOP/buckets", "$env:SCOOP_GLOBAL/apps" -Force | Out-Null
    Copy-Item "$scoopSource/*" $scoopCopy -Recurse -Force
    # Prefer Scoop's isolated path variable where the installed version supports it.
    '{"use_isolated_path": true, "use_external_7zip": true}' |
        Set-Content (Join-Path $env:SCOOP 'config.json') -Encoding UTF8
    $scoop = Join-Path $scoopCopy 'bin/scoop.ps1'
    & $scoop install $manifestPath --no-update-scoop --arch $Architecture
    if ($LASTEXITCODE -ne 0) { throw 'Scoop installation failed' }
    $installed = Join-Path $env:SCOOP 'apps/markview/current/markview.exe'
    $target = if ($Architecture -eq 'arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
    & "$PSScriptRoot/verify_windows_image.ps1" $installed $target
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    $actualTarget = $link.TargetPath
    # Let the shell normalize the expected target, including Windows short paths.
    $link.TargetPath = $installed
    if ($actualTarget -ne $link.TargetPath) { throw "Unexpected shortcut target: $actualTarget" }
    $shim = Join-Path $env:SCOOP 'shims/markview.exe'
    if (!(Test-Path $shim)) { throw 'Scoop did not create the command shim' }
    if ($Architecture -eq '64bit' -or $env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
        $process = Start-Process $shim -ArgumentList '--help' -WindowStyle Hidden -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "Installed command failed: $($process.ExitCode)" }
    }
    & $scoop uninstall markview
    if ($LASTEXITCODE -ne 0) { throw 'Scoop uninstallation failed' }
    foreach ($path in $installed, $shortcut, $shim) {
        if (Test-Path $path) { throw "Scoop left an installed file: $path" }
    }
    Write-Output "PASS Scoop $Architecture hash, installation, executable, shortcut, shim and uninstallation"
} finally {
    $testShims = Join-Path $work 'scoop/shims'
    foreach ($name in 'PATH', 'SCOOP_PATH') {
        $value = [Environment]::GetEnvironmentVariable($name, 'User')
        if ($value -and $value.Split(';') -contains $testShims) {
            $clean = ($value.Split(';') | Where-Object { $_ -ne $testShims }) -join ';'
            [Environment]::SetEnvironmentVariable($name, $clean, 'User')
        }
    }
    if (Test-Path $shortcut) { Remove-Item $shortcut -Force }
    if (!$hadShortcutDirectory -and (Test-Path $shortcutDirectory) -and
        @(Get-ChildItem $shortcutDirectory -Force).Count -eq 0) {
        Remove-Item $shortcutDirectory
    }
    if (Test-Path $work) {
        # Scoop marks its junctions read-only; unlink them before their targets.
        Get-ChildItem $work -Directory -Recurse -Force |
            Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint } |
            ForEach-Object {
                $_.Attributes = $_.Attributes -band (-bnot [IO.FileAttributes]::ReadOnly)
                [IO.Directory]::Delete($_.FullName)
            }
        Remove-Item $work -Recurse -Force
    }
}
