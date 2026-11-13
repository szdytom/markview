# Reject an emulated executable even when the package manager reports success.
$ErrorActionPreference = 'Stop'
$work = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
$previousProgramFiles = $env:ProgramFiles
$previousExitCode = $global:LASTEXITCODE
$state = @{}
function winget { $global:LASTEXITCODE = 0 }
function Get-ItemProperty {
    param([string]$Path)
    if ($Path -ne 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*') { throw 'unexpected registry lookup' }
    return [pscustomobject]@{ DisplayName = 'markview'; Publisher = 'szdytom'; PSChildName = '{11111111-1111-1111-1111-111111111111}' }
}
function Start-Process {
    param([string]$FilePath, [string]$ArgumentList, [switch]$Wait, [switch]$PassThru)
    if ($FilePath -eq 'msiexec.exe') {
        $state.Uninstalled = $true
    } else {
        if ($ArgumentList -ne '--help') { throw 'unexpected executable arguments' }
        $state.Launched = $true
    }
    return [pscustomobject]@{ ExitCode = 0 }
}
try {
    $env:ProgramFiles = $work
    $bin = Join-Path $work 'markview/bin'
    New-Item -ItemType Directory $bin -Force | Out-Null
    foreach ($machine in 0x8664, 0xAA64) {
        $bytes = [byte[]]::new(256)
        [BitConverter]::GetBytes([uint16]0x5A4D).CopyTo($bytes, 0)
        [BitConverter]::GetBytes([int32]0x80).CopyTo($bytes, 0x3C)
        [BitConverter]::GetBytes([uint32]0x00004550).CopyTo($bytes, 0x80)
        [BitConverter]::GetBytes([uint16]$machine).CopyTo($bytes, 0x84)
        [BitConverter]::GetBytes([uint16]0x20B).CopyTo($bytes, 0x98)
        [BitConverter]::GetBytes([uint16]2).CopyTo($bytes, 0xDC)
        [IO.File]::WriteAllBytes((Join-Path $bin 'markview.exe'), $bytes)
        foreach ($target in 'x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc') {
            $state.Launched = $false
            $state.Uninstalled = $false
            $accepted = $true
            try { & "$PSScriptRoot/verify_winget_package.ps1" -Target $target } catch { $accepted = $false }
            $expected = ($machine -eq 0xAA64) -eq $target.StartsWith('aarch64')
            if ($accepted -ne $expected -or $state.Launched -ne $expected) { throw "incorrect architecture accepted: $machine / $target" }
            if (-not $state.Uninstalled) { throw 'installed product was not cleaned up' }
        }
    }
    Write-Output 'PASS native WinGet executable checks and cleanup after architecture mismatch'
} finally {
    $env:ProgramFiles = $previousProgramFiles
    $global:LASTEXITCODE = $previousExitCode
    Remove-Item $work -Recurse -Force
}
