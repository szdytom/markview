# Check the architecture and GUI subsystem of a shipped Windows executable.
param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][ValidateSet("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")][string]$Target
)
$ErrorActionPreference = "Stop"
$bytes = [IO.File]::ReadAllBytes((Resolve-Path $Path).Path)
if ([BitConverter]::ToUInt16($bytes, 0) -ne 0x5A4D) { throw "$Path is not a PE image" }
$pe = [BitConverter]::ToInt32($bytes, 0x3C)
if ([BitConverter]::ToUInt32($bytes, $pe) -ne 0x00004550) { throw "$Path has no PE header" }
$machine = if ($Target -eq "aarch64-pc-windows-msvc") { 0xAA64 } else { 0x8664 }
if ([BitConverter]::ToUInt16($bytes, $pe + 4) -ne $machine) { throw "$Path is not a $Target image" }
if ([BitConverter]::ToUInt16($bytes, $pe + 24) -ne 0x20B) { throw "$Path is not a PE32+ image" }
if ([BitConverter]::ToUInt16($bytes, $pe + 0x5C) -ne 2) { throw "$Path is not a Windows-subsystem image" }
