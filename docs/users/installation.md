# Installation

[User documentation](README.md) · [Documentation](../README.md)

Download the latest build from [Releases](https://github.com/szdytom/markview/releases):

| Platform | Packages |
|:--|:--|
| Linux (x64, ARM64) | `.deb`, AppImage, `.tar.gz` |
| Windows (x64, ARM64) | `.msi`, `.zip` |
| macOS | zipped `.app` bundle |

Windows users can check the community package with `winget search --id szdytom.Markview --exact`. When available, install or update with:

```powershell
winget install --id szdytom.Markview --exact --source winget
winget upgrade --id szdytom.Markview --exact --source winget
```

On Linux and macOS, install the `markview` command with the Shell script:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/szdytom/markview/releases/latest/download/markview-installer.sh | sh
```

On Windows, install the `markview` command with PowerShell:

```powershell
irm https://github.com/szdytom/markview/releases/latest/download/markview-installer.ps1 | iex
```

For Windows on Arm, choose the `aarch64-pc-windows-msvc` MSI or portable ZIP.
The PowerShell installer selects ARM64 automatically with PowerShell 7.3+ or
Windows PowerShell on Windows 11 22H2+.

For ARM64 Linux, choose the `arm64.deb`, `aarch64.AppImage` or
`aarch64-unknown-linux-gnu.tar.gz` package. The Shell installer selects ARM64
automatically. Prebuilt Linux packages cover x64 and ARM64; 32-bit ARM requires
a source build.

Arch Linux users can install the prebuilt [AUR package](https://aur.archlinux.org/packages/markview-bin):

```sh
yay -S markview-bin
```

The [source package](https://aur.archlinux.org/packages/markview) builds locally:

```sh
yay -S markview
```

Linux builds need glibc 2.35 or newer, `libfontconfig1`, a working graphics
driver, a desktop portal for file dialogs, and Zenity for error dialogs.
ARM Linux builds enable Vulkan and GLES by default: Vulkan is tried first,
with GLES as a fallback. Linux x64 builds require Vulkan.
The macOS bundle is ad-hoc signed but not notarized. Move it to Applications;
if Gatekeeper blocks it, allow it in System Settings → Privacy & Security, or
clear its quarantine flag:

```sh
xattr -d com.apple.quarantine /Applications/Markview.app
```

The community-maintained [Homebrew tap](https://github.com/Stevvven777/homebrew-tap)
offers the Apple Silicon bundle:

```sh
brew install --cask Stevvven777/tap/markview
```

The Windows MSI adds Markview to the **Open with** list for `.md`, `.markdown`
and `.mdown` and lists it under **Default apps**. Windows 10 and 11 still ask the
user to confirm the handoff, so the first one of those files is a choice, not
something an installer can make on the user's behalf.

The macOS bundle answers the desktop the same way: a Markdown file
double-clicked in Finder, or chosen under **Open with**, opens in the reader,
and so does one dropped on the app's icon.

The Android APK is available from [Releases](https://github.com/szdytom/markview/releases).
See [Android reading](android.md) for document handling and mobile controls.

## Portable mode

Windows ZIP and Linux extracted builds can keep their data beside the executable.
Close Markview, then create `markview-portable-marker.txt` next to `markview.exe`
or `markview`. On Windows, run the ZIP's `enable-portable.bat` to create it.
The marker's contents do not matter. The BAT only creates the marker; it does
not launch Markview or copy existing settings.

On the next launch, Markview uses a `data/` directory beside the executable for
settings, sessions, history, styles, downloaded fonts, image cache and instance
communication files. The directory must be writable; a write failure does not
switch back to system configuration. Each portable copy has its own data and
window discovery. Temporary files still use the operating system's temporary
directory. macOS and Android do not support this mode.

Existing settings are not migrated automatically. To keep them, close Markview
and copy the contents of the [system settings directory](settings.md#settings-file)
into `data/`. Move the executable, marker and `data/` together to carry your
configuration. Paths to documents or external resources retain their existing
meaning; moving the reader does not rewrite them.

To stop using portable mode, close Markview and remove the marker. The next
launch uses system configuration again; the portable data remains available.
ZIPs do not enable portable mode until you create the marker.

## Runtime requirements

The Linux artifacts rely on host components on purpose: bundling glibc is the
usual source of AppImage breakage, and graphics drivers and system fonts
cannot be shipped meaningfully.

| Platform | Required |
| --- | --- |
| Linux | glibc 2.35+, `libfontconfig1`, Vulkan (`libvulkan1` and a working driver) or, on ARM, GLES 3.0+ (`libegl1`, `libgles2` and a working driver); X11 or Wayland client libraries, `xdg-desktop-portal` with a backend, `zenity` for error dialogs, and system fonts |
| Windows | Windows 10 or newer with a Direct3D 12 driver; the MSVC runtime is linked statically |
| macOS | macOS 11 or newer on Apple Silicon |

The Debian package declares these as `Depends`, so `apt` resolves them. The
AppImage declares nothing and shows a wgpu backend error when no compatible
graphics driver is present. ARM Debian packages also install the EGL and GLES
loaders for fallback. `fonts-noto-cjk` is a recommendation rather than a requirement,
because Markview falls back to whatever CJK faces the system provides.

When multiple GPU backends are compiled in, Markview tries them one at a time
until surface, adapter and device initialization succeeds. The default order
is Vulkan, Metal, Direct3D 12, WebGPU, then OpenGL/GLES. Set `WGPU_BACKEND` to
a comma-separated list to restrict the backends and choose their order, for
example `WGPU_BACKEND=gl,vulkan` tries OpenGL/GLES before Vulkan. Backends
absent from the build are skipped; the variable cannot enable them. If every
attempt fails, the error includes each backend's failure.
