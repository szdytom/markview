# Android development

[Developer documentation](README.md) · [Android user guide](../users/android.md)

**MV4A — MarkView as a Android App** runs Markview's desktop application on
Android 11 (API 30) and newer. It uses the existing Rust `App`, native typography
and GPU renderer. Android supplies the activity, system file pickers, clipboard
and URI permissions.

The installed app is named **Markview**. **MV4A** is used only in documentation
to distinguish this Android subproject from the Markview desktop app and MVaaC.

## What is shared

| Capability | Existing implementation used by MV4A |
|---|---|
| Settings, styles and languages | `src/app/chrome`, `src/settings.rs`, `src/stylesheet.rs` |
| Tabs and reading positions | `src/state`, `src/app/tab_strip.rs`, `src/app/tab_metrics.rs` |
| Font catalogue, downloads and family selection | `src/fonts.rs`, `src/app/font_panel`, shared font configuration |
| Images, SVG and Mermaid | `src/images`, including bounded decoding and the persistent HTTP cache |
| Markdown, CJK and mathematics | `markview-layout`, the desktop worker and layout pipeline |
| Search, outline, gestures and image viewer | The desktop application controllers |
| PDF and PNG export | `src/app/export.rs`, `markview-pdf`, the desktop renderer |

The APK contains the root `markview` crate as `libmarkview.so`. The Android
entry registration is in `src/lib.rs`, with reader setup in `src/app/android.rs`;
JNI and OS calls live in `crates/markview-platform/src/android.rs` and
`android/java`. The [pinned `winit` fork](https://github.com/szdytom/winit/commit/9299a76998fd8975afb97c5a1d81495845ab8ec8)
handles Activity destruction and allows event-loop recreation. This shares the
application above `core`, including its state and UI. See the [Android user guide](../users/android.md) for phone/tablet behavior and screenshots.

Android reuses the `singleTask` activity.

Upstream tracking: [Destroy handling #4303](https://github.com/rust-windowing/winit/issues/4303),
[event-loop recreation #3325](https://github.com/rust-windowing/winit/issues/3325),
and [Destroy fix #4711](https://github.com/rust-windowing/winit/pull/4711).
Return to a published upstream release once it includes both lifecycle fixes
and passes the phone and tablet Activity integration tests.

## Build and install

Use a recent Rust toolchain, Python 3.11 or newer, JDK 17 or newer, and the Android SDK.
The current script supports Linux and macOS build hosts. Install SDK command
line tools, accept the SDK licences, then install:

```sh
sdkmanager 'platform-tools' 'platforms;android-35' 'build-tools;35.0.0' 'ndk;29.0.14206865'
rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_HOME=/path/to/android-sdk
python3 android/build.py
adb install -r target/android/markview-android-debug.apk
```

`ANDROID_NDK_HOME` overrides the NDK location. If `ANDROID_HOME` is absent, the
script uses `.tools/android-sdk` in the repository. `--abi arm64-v8a` and
`--abi x86_64` build one architecture; debug APKs contain both by default, while
release APKs default to ARM64 (`arm64-v8a`, aarch64). `--abi all` includes both
architectures in either build profile for testing. Native
libraries and APK entries are aligned for 16 KiB pages.

`--release` enables Rust optimizations, disables Android debugging and writes
`target/android/markview-android-release.apk`. Without `--keystore`, both build
variants are signed with a local development key in `target/android`.
The APK version name follows the workspace version in `Cargo.toml`;
`--version-code` sets the Android update counter (default: `1`). SDK files,
keys, native libraries and APKs are excluded from Git.

## GitHub Actions and releases

Every push and pull request runs [Android](../../.github/workflows/android.yml)
through the main CI workflow. It builds an x86_64 debug APK once, then runs the
integration suite on API 35 AOSP Pixel 6 and Pixel Tablet emulators. Each device
also checks its side of the `sw599dp` / `sw600dp` boundary. Both run headlessly
with KVM and Mesa software Vulkan, using the pinned emulator 36.1.8 rather than
its incompatible SwiftShader backend. The workflow can also be run manually.
CI allows ten minutes per full instrumentation run for software rendering;
local runs keep the three-minute default, adjustable with `--timeout`.
CI emulators use four cores and 4 GiB RAM. AOSP images omit Google services.
Instrumentation closes ANR dialogs from Quickstep or Pixel Launcher and waits
for Markview to regain focus before reader touches. The test runner grants the
app All files access with `appops` and writes its folder fixture under shared
storage.
CI explicitly selects gesture navigation on both devices.
Device tests wait for viewport and reading-width layout to settle before assertions.
Native inspection retries empty replies while rendering or resuming; system-bar
pixel checks include the native window's screen offset. Instrumentation disables
GPU debug labels to avoid `vulkan.ranchu` crashes when naming swapchain views,
while retaining GPU validation.
Instrumentation intercepts PDF and PNG preview intents after publication,
so external viewers cannot capture subsequent reader input.
`markview-android-debug` keeps the APK and instrumentation build inputs;
`markview-android-tests-phone` and `markview-android-tests-tablet` keep reports,
screenshots and logcat, including on failure. Android must pass for `ci` to pass.

The existing cargo-dist Release workflow runs the same Android tests before
publication and calls [Android release package](../../.github/workflows/android-release.yml)
through Packaging. A successful version-tag release includes
`markview-<version>-android.apk` (ARM64 only), with an Android row in the download table.
The Release workflow's run number supplies the APK version code; retain that
workflow's counter so later APKs can upgrade earlier installations.
Pull requests build release APKs with a separate, generated development key and
do not publish them. Distribution signing secrets are only passed to the tag
release signing step; PR builds never receive them.

Before the first tag release, configure these repository Actions secrets:

| Secret | Value |
|---|---|
| `ANDROID_KEYSTORE_BASE64` | Base64-encoded distribution keystore |
| `ANDROID_KEYSTORE_PASSWORD` | Keystore password |
| `ANDROID_KEY_ALIAS` | Signing key alias in the keystore |
| `ANDROID_KEY_PASSWORD` | Signing key password |

Create the keystore once, keep a backup, and reuse it across releases so users
can install updates. For example, `keytool` prompts for passwords:

```sh
keytool -genkeypair -keystore markview-release.keystore -alias markview \
  -keyalg RSA -keysize 4096 -validity 10000 -dname 'CN=Markview'
base64 < markview-release.keystore | tr -d '\n'
```

Use the encoded output only for `ANDROID_KEYSTORE_BASE64`. Release signing
requires all four secrets; missing credentials fail the release before
publication. The keystore is decoded into the runner's temporary directory,
removed after signing, and excluded from uploaded release artifacts.
For a local distribution build, set `ANDROID_KEYSTORE_PASSWORD` and
`ANDROID_KEY_PASSWORD` in the environment, then run:

```sh
python3 android/build.py --release --version-code 42 \
  --keystore /path/to/markview-release.keystore --key-alias markview
```

This publishes installable APKs on GitHub Releases. Google Play publication
is a separate process.

## Emulator verification

Create an API 35 x86_64 AVD and boot it with a working GPU backend:

```sh
sdkmanager 'emulator' 'system-images;android-35;google_apis;x86_64'
avdmanager create avd -n markview-api35 -k 'system-images;android-35;google_apis;x86_64' --device pixel_6
emulator -avd markview-api35 -gpu host -no-snapshot -no-window
python3 android/build.py --abi x86_64
python3 android/test.py --serial emulator-5554 --layout phone
```

For tablet verification, create an AVD with `--device pixel_tablet`, boot it on
a separate port, and run `python3 android/test.py --serial emulator-5556 --layout tablet`.
`--layout-only` checks resource selection and orientation policy for boundary
configurations such as `sw599dp` and `sw600dp`.

The test APK supplies external content URIs and exercises real touch and key
input in the rendered reader. It checks multilingual Markdown, mathematics,
Mermaid and image decoding, scrolling and cached tabs, shared settings, font
catalogue pages, theme selection, search input, phone portrait lock,
phone drawer operations, tablet rotation, background/resume, durable preferences,
the system picker, direct shared-storage folder reads and live external changes, read-only grants, PDF saving,
GPU PNG export, and repeated Activity destruction/recreation in the same process.
Use `--lifecycle-only` to run just the Activity checks, including Android
**Don't keep activities**. Run both devices headlessly with `-no-window`, as shown above.
Use `--session-only` to verify background saves and tab/reading-position restoration
after a process restart, including external-open deduplication.
Use `--mermaid-only` to verify all three diagram labels have visible pixels in
light and dark styles, including Android's variable Roboto Flex font.
Use `--links-only` to verify cold-start web opening, URL sharing and tab reuse
against a local HTTP article fixture, without public Internet access. Web-link
tests use `adb root` on the emulator to route a benchmarking address to loopback,
keeping the reader's private-address restrictions enabled.
Reports and screenshots are written to `artifacts/android/phone/` and
`artifacts/android/tablet/` (or the corresponding `*-boundary/` directories).

By default the test seeds a valid image-cache fixture from the repository logo
so it remains repeatable without public Internet access. `--online` instead
clears that entry and fetches the image from GitHub. HTTP cache fetching,
validation and offline behaviour are also covered by the shared workspace
tests. Tests run against a debug APK; the inspection JNI entry point is absent
from release builds.

For this development environment, emulator 36.1.8 with `-gpu host` boots and
renders through the Vulkan backend. Emulator 37.2.12 failed before guest boot,
and 36.1.8's SwiftShader backend failed during shader creation. These host
emulator limitations do not change the APK's Vulkan/OpenGL fallback.

Run the desktop regression and GPU export checks with:

```sh
cargo test --workspace --locked
cargo test --lib a_whole_document_png_export_stitches_its_tiles
cargo clippy --workspace --all-targets --locked -- -D warnings
```
