# Develop on Android

Markview uses the shared Rust reader, layout and renderer on Android. Platform
services and JNI belong in `markview-platform` and `android/java`; application
lifecycle adaptation belongs in the root app. See [user behavior](../users/android.md)
for the distinction between imported single files and direct folder access.

The `winit` fork is pinned in Cargo configuration for Activity destruction and
event-loop recreation. An upstream migration must pass phone and tablet lifecycle
tests, including repeated destruction/recreation in one process.

## Build and install

Use Python 3.11 or newer, JDK 17 or newer, and the Android SDK. Android builds
select Rust 1.100.0 explicitly. `[workspace.metadata.android]` in
[`Cargo.toml`](../../Cargo.toml) pins Rust, the Java language level, Build Tools,
NDK and `cargo-about`; the SDK levels come from `android/AndroidManifest.xml`.
Local builds, instrumentation builds and CI read these values through
`python3 android/config.py`. CI uses OpenJDK 17 (Temurin).
Application Java sources, instrumentation tests and host JVM tests target Java 17.
The minimum Android version remains Android 11 (API 30). Java APIs must be
available on API 30 or guarded by a platform version check; `--release 17`
does not enforce Android API availability. The build invokes D8 directly and
does not configure core library desugaring to backport newer Java APIs.

The current script supports Linux and macOS build hosts. Install SDK command
line tools, accept the SDK licences, then install:

```sh
export ANDROID_HOME="$HOME/Android/Sdk"
sdkmanager --sdk_root="$ANDROID_HOME" 'platform-tools' 'platforms;android-35' 'build-tools;35.0.0' 'ndk;29.0.14206865'
rustup toolchain install 1.100.0 --profile minimal --target aarch64-linux-android --target x86_64-linux-android
cargo +1.100.0 install cargo-about --version 0.9.2 --locked --features cli
python3 android/build.py
adb install -r target/android/markview-android-debug.apk
```

`ANDROID_NDK_HOME` overrides the NDK location. If `ANDROID_HOME` is absent, the
script uses `~/Android/Sdk`, shared across repository checkouts. `--abi arm64-v8a` and
`--abi x86_64` build one architecture; debug APKs contain both by default, while
release APKs default to ARM64 (`arm64-v8a`, aarch64). `--abi all` includes both
architectures in either build profile for testing. Native
libraries and APK entries are aligned for 16 KiB pages.

On NixOS, run the official NDK in an FHS environment or patch its host executable
interpreter and library paths for Nix. This host setup is separate from the APK
build; the official F-Droid build environment uses Debian.

`--release` enables Rust optimizations, disables Android debugging and writes
`target/android/markview-android-release.apk`. Unless `--unsigned` is set, builds
without `--keystore` use a local development key in `target/android`.
The APK version name follows the workspace version in `Cargo.toml`.
The Android update counter defaults to `[workspace.metadata.android].version-code`
in `Cargo.toml`; increment it for every tagged release, including prereleases.
The initial value `195` matches the published `v0.5.2` APK. GitHub and F-Droid
must use this same source value; `--version-code` is an explicit override for
packaging and tests. The source manifest omits version attributes because the
build writes both from Cargo configuration. SDK files,
keys, native libraries and APKs are excluded from Git.

For external signing, including F-Droid's own signing service, run:

```sh
python3 android/build.py --release --unsigned
```

This writes `target/android/markview-android-release-unsigned.apk`, aligned for
16 KiB pages, without creating or using a keystore. `--unsigned` requires
`--release` and cannot be combined with `--keystore`. It is a build preparation
option; Markview is not yet listed in the official F-Droid repository.

## Dependency licenses

Every APK includes `assets/licenses/LICENSE`, `THIRD_PARTY.md`, the resource
licenses under `licenses/`, and generated `third-party-notices.html`.
Upstream `NOTICE*` and `COPYRIGHT*` files from the selected dependencies are
preserved under `assets/licenses/dependencies/<name>-<version>/`.
`cargo-about` checks the selected Android targets and default reader features,
excluding dependencies used only for building or tests. The existing
[`about.toml`](../../about.toml) acceptance policy applies; an unsupported or
unresolved license fails the APK build. The notices reproduce dependency license
texts and link to exact source packages for the unmodified MPL-2.0 components.

The KaTeX math fonts embedded by `ratex-katex-fonts` use OFL-1.1; the Lucide icon
geometry uses ISC. Noto and Roboto Flex subsets are test fixtures, not APK assets.
User-triggered stylesheet font downloads are separate from APK packaging.
See [third-party components](../../THIRD_PARTY.md) for the component summary.

Dependencies are pinned by `Cargo.lock`; the `winit`, `dom_smoothie` and Mermaid
forks use exact Git revisions in `Cargo.toml`. New dependencies and fork updates
still need source and license review. Passing `cargo-about` does not replace
F-Droid's source/binary scan or inclusion review.

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

Run both phone and tablet configurations. Use `--layout-only` for the
`sw599dp`/`sw600dp` boundary, `--lifecycle-only` for Activity recreation and
`--session-only` for process-restart restoration. Session tests are opt-in;
`--filter '*'` includes them. Use `--list` to inspect available cases.

List, filter or shuffle cases without changing the suite:

```sh
python3 android/test.py --list
python3 android/test.py --serial emulator-5554 --filter SearchTest
python3 android/test.py --serial emulator-5554 --filter '*Settings*' --filter '*Tabs*'
python3 android/test.py --serial emulator-5554 --shuffle 42
```

Filters are case-sensitive shell globs over class names. Repeated filters form
a union; an empty match fails before building or contacting the device.
Add new cases by extending `AndroidTest` and registering their instrumentation
in `android/tests/AndroidManifest.xml`; the host discovers that manifest directly.
Use `openReader()` and fixture helpers to prepare prerequisites within the case.
Send keys with `pressKey()` so launcher ANR recovery runs before input;
pass `"documentsui"` as the second argument when cancelling a system picker.
Do not rely on another class to change settings, open tabs or create files.

Reports and screenshots are written under `artifacts/android/phone/<ClassName>/`
and `artifacts/android/tablet/<ClassName>/`, with a suite summary in
`integration.txt`. Legacy shortcuts use the corresponding `*-boundary`,
`*-lifecycle`, `*-mermaid`, `*-links` or `*-session` directory. Screenshots are
collected from each case rather than a fixed list. A failed or timed-out case
keeps its report and does not prevent the remaining selected cases from running.

Run the host orchestration regression tests without an emulator:

```sh
python3 -m unittest discover -s android -p 'test_*.py'
```

By default the test seeds a valid image-cache fixture from the repository logo
so it remains repeatable without public Internet access. `--online` instead
clears that entry and fetches the image from GitHub. HTTP cache fetching,
validation and offline behaviour are also covered by the shared workspace
tests. Tests run against a debug APK; the inspection JNI entry point is absent
from release builds.

## GitHub Actions and releases

The [Android workflow](../../.github/workflows/android.yml) owns SDK/emulator pins,
phone/tablet execution and artifacts. Use its software-rendering setup when host
GPU behavior differs. Reports, screenshots and logcat are required evidence for
an emulator failure; a desktop test pass is not Android verification.

The [release workflow](../../.github/workflows/android-release.yml) builds the
ARM64 distribution APK. Keep the signing key and version-code progression stable
so an installed app can upgrade. PR builds use development signing and must not
receive distribution secrets.

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
python3 android/build.py --release \
  --keystore /path/to/markview-release.keystore --key-alias markview
```

This publishes installable APKs on GitHub Releases. Google Play publication
is a separate process.

## JNI errors

JNI entry points use `EnvUnowned::with_env` to contain Rust panics. Recoverable
errors become contextual Java exceptions; a caught Rust panic becomes a
`NativePanic` exception, which ends the reader. Rust-to-Java calls retain the Java
exception message and stack trace. UI tasks catch errors inside their runnable,
including picker failures that must cancel the outstanding native request.

Errors appear in system `AlertDialog` widgets with selectable, scrollable
details, app version, device model, Android version and ABI. Fatal errors use a
Java-only `ErrorActivity`, so the message remains after `NativeActivity` closes.
Users can send a screenshot; full details also go to logcat under `Markview`.
Reports are not persisted or uploaded. Process termination by a native signal,
driver crash or the OS cannot reliably display a dialog.

Run `BridgeTest`, `JniPanicTest`, `ReaderFailureTest` and `ReaderPanicTest` to
exercise injected JNI errors, Java exceptions, failed string returns, request
cancellation, caught panics and fatal error presentation. The failure-injection native export is
absent from release builds.
