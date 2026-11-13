# Installation

[User documentation](README.md) · [Documentation](../README.md)

Download the latest build from [Releases](https://github.com/szdytom/markview/releases):

| Platform | Packages |
|:--|:--|
| Linux | `.deb`, AppImage, `.tar.gz` |
| Windows (x64, ARM64) | `.msi`, `.zip` |
| macOS | zipped `.app` bundle |

Once the [WinGet community submission](https://github.com/microsoft/winget-pkgs/pull/445697) is accepted, Windows users can install and update it with:

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

Arch Linux users can install the prebuilt [AUR package](https://aur.archlinux.org/packages/markview-bin):

```sh
yay -S markview-bin
```

The [source package](https://aur.archlinux.org/packages/markview) builds locally:

```sh
yay -S markview
```

Linux builds need glibc 2.35 or newer, `libfontconfig1`, a working Vulkan
driver, a desktop portal for file dialogs, and Zenity for error dialogs.
The macOS bundle is unsigned, so
clear the quarantine flag once after downloading it:

```sh
xattr -d com.apple.quarantine /Applications/Markview.app
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

## Runtime requirements

The Linux artifacts rely on host components on purpose: bundling glibc is the
usual source of AppImage breakage, and the Vulkan driver and system fonts
cannot be shipped meaningfully.

| Platform | Required |
| --- | --- |
| Linux | glibc 2.35+, `libfontconfig1`, `libvulkan1` (loader plus any working Vulkan driver), X11 or Wayland client libraries, `xdg-desktop-portal` with a backend, `zenity` for error dialogs, and system fonts |
| Windows | Windows 10 or newer with a Direct3D 12 driver; the MSVC runtime is linked statically |
| macOS | macOS 11 or newer on Apple Silicon |

The Debian package declares these as `Depends`, so `apt` resolves them. The
AppImage declares nothing and shows a wgpu backend error when no Vulkan driver
is present. `fonts-noto-cjk` is a recommendation rather than a requirement,
because Markview falls back to whatever CJK faces the system provides.
