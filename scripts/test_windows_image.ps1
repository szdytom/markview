$ErrorActionPreference = "Stop"
$path = Join-Path ([IO.Path]::GetTempPath()) "$([guid]::NewGuid()).exe"
$verify = Join-Path $PSScriptRoot "verify_windows_image.ps1"
try {
    foreach ($machine in 0x8664, 0xAA64) {
        foreach ($target in "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc") {
            foreach ($subsystem in 2, 3) {
                $bytes = [byte[]]::new(256)
                [BitConverter]::GetBytes([uint16]0x5A4D).CopyTo($bytes, 0)
                [BitConverter]::GetBytes([int32]0x80).CopyTo($bytes, 0x3C)
                [BitConverter]::GetBytes([uint32]0x00004550).CopyTo($bytes, 0x80)
                [BitConverter]::GetBytes([uint16]$machine).CopyTo($bytes, 0x84)
                [BitConverter]::GetBytes([uint16]0x20B).CopyTo($bytes, 0x98)
                [BitConverter]::GetBytes([uint16]$subsystem).CopyTo($bytes, 0xDC)
                [IO.File]::WriteAllBytes($path, $bytes)
                $expected = $subsystem -eq 2 -and (($machine -eq 0xAA64) -eq $target.StartsWith("aarch64"))
                $accepted = $true
                try { & $verify $path $target } catch { $accepted = $false }
                if ($accepted -ne $expected) { throw "unexpected result: machine=$machine target=$target subsystem=$subsystem" }
            }
        }
    }
    # Invalid signatures, optional headers and truncated images must fail.
    [BitConverter]::GetBytes([uint16]2).CopyTo($bytes, 0xDC)
    $invalid = @([byte[]]::new(2))
    foreach ($offset in 0, 0x80, 0x98) {
        $copy = $bytes.Clone()
        $copy[$offset] = 0
        $invalid += ,$copy
    }
    foreach ($data in $invalid) {
        [IO.File]::WriteAllBytes($path, $data)
        $accepted = $true
        try { & $verify $path "aarch64-pc-windows-msvc" } catch { $accepted = $false }
        if ($accepted) { throw "invalid PE image was accepted" }
    }
    Write-Output "PASS Windows executable architecture and subsystem checks"
} finally {
    Remove-Item $path
}
