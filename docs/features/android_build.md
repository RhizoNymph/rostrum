# Feature: android_build

The Android app's toolchain and build pipeline. A Kotlin/Jetpack Compose app
under `android/` calls the shared Rust crates through UniFFI. One Gradle build
cross-compiles the Rust library for each Android ABI, generates its Kotlin
bindings, packages both into a signed release APK, and a script publishes that
APK for `rostrumd` to serve to phones.

## Scope

- The Gradle project under `android/` (one `app` module), its pinned
  toolchain (Gradle wrapper, AGP, Kotlin, the JDK the daemon runs on) and its
  version catalog.
- Building `librostrum_ffi.so` for `arm64-v8a` and `x86_64` with cargo-ndk,
  generating the Kotlin bindings with the crate's own `uniffi-bindgen`, and
  packaging both, plus JNA's `libjnidispatch.so`, into the APK.
- The `rostrum-ffi` crate's build contract: crate type, the `uniffi-bindgen`
  binary, `uniffi.toml`, and the `android-release` cargo profile. Its exports
  are a placeholder (`ffi_version`) that proves the path links end to end.
- Release signing: a keystore outside the repository, credentials in the
  gitignored `android/.env` or the environment, and a debug-key fallback.
- `android/scripts/build-apk.sh` and `android/scripts/publish-apk.sh`, and the
  JSON metadata contract with `rostrumd`.
- The UI foundation that every screen builds on: `RostrumColors`, the Material
  3 colour scheme mapped from it, and the bundled fonts.

## Non-scope

- **Screens, navigation, secrets, pairing and notifications.** Those are the
  app itself, documented in `docs/features/android_app.md`. This feature only
  declares the `rostrum://pair` intent filter they rely on.
- **The real FFI surface.** The exports of `rostrum-ffi` are replaced later;
  this feature fixes only the crate/bin/package names the build relies on.
- **Serving the APK.** `rostrumd` serves `~/.local/share/rostrum/server/apk/`.
  This feature only fills that directory.
- 32-bit ABIs (`armeabi-v7a`, `x86`), App Bundles, Play Store upload, CI.
- **Running the APK.** There is no emulator or device on the build machine, so
  this feature verifies the APK statically (contents, R8 output, signature,
  alignment), not at runtime.

## Prerequisites

`android/scripts/build-apk.sh` checks all of these and names the fix for any
that is missing.

| What | Version | Where (on the development machine) |
|---|---|---|
| Rust targets `aarch64-linux-android`, `x86_64-linux-android` | for the active (nightly) toolchain | `~/.rustup` via `rustup target add` |
| cargo-ndk | 4.1.2 (`cargo install cargo-ndk --version 4.1.2 --locked`) | `~/.cargo/bin/cargo-ndk` |
| Android NDK | r30, `30.0.16248370` (the `ndk` entry of `gradle/libs.versions.toml`) | `~/Android/Sdk/ndk/30.0.16248370`, installed with `sdkmanager --install 'ndk;30.0.16248370'` |
| Android SDK | platform `android-36`, build-tools (any; the scripts use the newest) | `~/Android/Sdk`, `sdk.dir` in `android/local.properties` |
| JDK 21 | the Gradle daemon's JVM (`gradle/gradle-daemon-jvm.properties`) and the compile toolchain | detected by Gradle, here the one it provisioned in `~/.gradle/jdks/` |
| Any Java | only to launch `gradlew` | system Java 25 (a JRE; it has no `javac`, which is why the daemon must not run on it) |

## Build flow

`android/scripts/build-apk.sh` runs the whole thing from a clean tree:

1. **Preflight (the script).** It resolves cargo and rustup (`$CARGO`, else
   `~/.cargo/bin/cargo`, never a PATH lookup; see *Invariants*) and checks both
   Rust targets and cargo-ndk. It writes `local.properties` from
   `$ANDROID_HOME`/`~/Android/Sdk` if the file is missing, then resolves the
   NDK the same way Gradle does and checks it. If `android/.env` is missing it
   is restored from `~/.config/rostrum/android/release.env`. Finally it runs
   `./gradlew :app:assembleRelease` with `CARGO` and `ANDROID_NDK_HOME`
   exported.
2. **Configuration (Gradle).** `buildSrc` is compiled first. It holds the task
   types and helpers in `rostrum.buildlogic`. `app/build.gradle.kts` then:
   - registers `cargoNdkBuild` (`CargoNdkBuild`) and `uniffiBindgen`
     (`UniffiBindgen`), and makes `preBuild` depend on both, so every variant's
     tasks run after them;
   - adds `app/build/rustJniLibs` to `main`'s jniLibs and
     `app/build/generated/source/uniffi/kotlin` to `main`'s Kotlin sources;
   - calls `ReleaseSigning.load(android/.env, env)` to choose the release
     signing config.
3. **`cargoNdkBuild`** runs in the workspace root:

   ```sh
   cargo ndk -t arm64-v8a -t x86_64 -P 28 -o android/app/build/rustJniLibs \
     build --lib --locked --profile android-release -p rostrum-ffi
   ```

   For each ABI, cargo-ndk points cargo at the NDK's clang for linking at API
   level 28 (minSdk) and runs the build through `$CARGO`, which the outer cargo
   sets to its real toolchain binary. It then copies
   `target/<triple>/android-release/librostrum_ffi.so` to
   `rustJniLibs/<abi>/`. `--lib` skips the `uniffi-bindgen` binary, which only
   runs on the host. The task first deletes its output directory, so an ABI
   or library dropped from the build cannot linger in the APK. Its inputs are the root
   `Cargo.toml`, `Cargo.lock`, and every file under `crates/`. Its `@Input`s
   are the cargo path, the NDK path, the package, the profile, the ABIs, and
   the API level. When Gradle decides it is out of date, cargo's own
   incremental build does the rest.
4. **`uniffiBindgen`** runs on the host:

   ```sh
   cargo run --locked -p rostrum-ffi --bin uniffi-bindgen -- generate \
     --library android/app/build/rustJniLibs/arm64-v8a/librostrum_ffi.so \
     --language kotlin --out-dir android/app/build/generated/source/uniffi/kotlin --no-format
   ```

   This is library mode. The interface comes from the `UNIFFI_META_*`
   symbols in the built `.so`, so the bindings always describe exactly the
   binary that ships. `uniffi.toml` is found next to the crate's manifest
   through `cargo metadata`. The output is
   `uniffi/rostrum_ffi/rostrum_ffi.kt`. The task's inputs are the arm64 `.so`,
   `Cargo.lock` (which fixes the uniffi version), and `uniffi.toml`.
5. **Kotlin compile.** AGP 9 compiles Kotlin itself (`android.builtInKotlin`);
   `org.jetbrains.kotlin.plugin.compose` adds the Compose compiler. App
   sources and the generated bindings compile together.
6. **R8** minifies and shrinks resources. `app/proguard-rules.pro` keeps
   `com.sun.jna.**` and all of `uniffi.**`, because JNA binds by reflection and
   by class name.
7. **Native libraries.** AGP merges `rustJniLibs/<abi>/librostrum_ffi.so`,
   `jni/<abi>/libjnidispatch.so` from the JNA AAR, and AndroidX's own. The
   `abiFilters` keep `arm64-v8a` and `x86_64` only. AGP then strips each
   library with the NDK's `llvm-strip` (the NDK is found from `ndkVersion`),
   which removes `.symtab`. `.dynsym`, which JNA's `dlsym` uses, stays.
8. **Packaging and signing.** The libraries are stored uncompressed with
   16 KB-aligned segments (NDK r30's default; checked with
   `zipalign -c -P 16`). The APK is signed with APK Signature Scheme v2 using
   the release key (minSdk 28 needs no v1), producing
   `android/app/build/outputs/apk/release/app-release.apk`.
9. **Publish** (`android/scripts/publish-apk.sh`, run separately):
   `apksigner verify`, refusing debug-signed APKs → `aapt2 dump badging` for
   `versionName`/`versionCode` → copy to a temp file in the destination →
   sha256 and size of that copy → JSON to a temp file → rename the APK, then
   the JSON.

At runtime the call path into Rust is `RustCore.probe()` →
`uniffi.rostrum_ffi.ffiVersion()`. On first use, the generated `UniffiLib`
object calls `Native.register(..., "rostrum_ffi")`. JNA loads
`libjnidispatch.so` and `librostrum_ffi.so` from the APK and the bindings
check the API checksums. The Rust function returns a `RustBuffer`, which the
bindings lift into a Kotlin `String`. A `LinkageError` anywhere in that
chain becomes `CoreLink.Unavailable`. While the app runs on the fake backend
(phase 1 of `android_app`), nothing calls the probe; the adapter over
`RostrumCore` replaces it.

## The Rust side: `rostrum-ffi`

- `crate-type = ["cdylib", "lib"]`. The `cdylib` is the `.so`; the `lib` is
  what the `uniffi-bindgen` binary and the tests link.
- Proc-macro scaffolding (`uniffi::setup_scaffolding!()`, `#[uniffi::export]`),
  with no UDL file. The namespace is the crate name, so the Kotlin package is
  `uniffi.rostrum_ffi` and the library JNA loads is `rostrum_ffi`.
- `[[bin]] uniffi-bindgen` calls `uniffi::uniffi_bindgen_main()`. It needs
  uniffi's `cli` feature, which is enabled only for
  `cfg(not(target_os = "android"))`. Resolver 2 does not unify features
  across targets, so the Android build never compiles `uniffi_bindgen`, clap,
  goblin, or askama. On Android the binary is a stub that exits with an error.
- `uniffi.toml` sets `[bindings.kotlin] android = true`. Generated objects then
  use `android.system.SystemCleaner` on API 34+ (JNA's cleaner below that)
  instead of probing for `java.lang.ref.Cleaner`, which Android lacks before
  API 33.
- `[profile.android-release]` in the root `Cargo.toml` inherits `release` and
  sets `opt-level = 3`, `lto = true`, `codegen-units = 1`,
  `panic = "unwind"`, `debug = false`, and `strip = "debuginfo"`. It is a
  separate profile so desktop `--release` builds keep their settings. To
  iterate faster on the Rust side, build with `-Prostrum.cargoProfile=dev`.

## Signing

- The keystore is `~/.config/rostrum/android/release.jks`: PKCS12, RSA 4096,
  SHA384withRSA, alias `rostrum`, valid for 30 years, generated with JDK 21's
  `keytool`. It is never inside the repository. Losing it means installed apps
  can no longer be updated in place.
- `android/.env` (gitignored, mode 0600) holds `ROSTRUM_KEYSTORE`,
  `ROSTRUM_KEYSTORE_PASSWORD`, `ROSTRUM_KEY_ALIAS`, and `ROSTRUM_KEY_PASSWORD`.
  An environment variable of the same name takes precedence. `.env.example`
  is the committed template. PKCS12 uses one password for the store and the
  key, so the two password settings are equal.
- A copy, `~/.config/rostrum/android/release.env`, sits next to the keystore,
  because a worktree's `android/.env` goes away with the worktree.
  `build-apk.sh` restores `android/.env` from it when missing.
- `ReleaseSigning.load` returns `Keystore` or `Unavailable(reason)` and never
  throws. If any setting is missing or empty, or the keystore file does not
  exist, the release build is signed with the debug key and Gradle prints
  `warning: release build is signed with the DEBUG key: <reason>`.
  `publish-apk.sh` refuses to publish such an APK, since phones could not
  install it over a release-signed one.

## The publish contract with `rostrumd`

`~/.local/share/rostrum/server/apk/` holds exactly:

- `rostrum.apk`, the release APK;
- `rostrum.apk.json`, a single line with exactly these keys and types:

  ```json
  {"version_name": "0.1.0", "version_code": 1, "built_at": "2026-09-28T09:11:58Z", "sha256": "<64 lowercase hex>", "size": 3237493}
  ```

  `version_name` and `version_code` are read from the APK with
  `aapt2 dump badging`, not from Gradle, so they describe the file. `built_at`
  is the APK's mtime in RFC 3339 UTC. `sha256` and `size` are taken from the
  copy that is served. `version_name` must match `[0-9A-Za-z._+-]+`, so it
  never needs JSON escaping.

Both files are written as temp files in that directory and renamed into place,
the APK first. A reader therefore never sees a partial file, and the JSON never
describes an APK that is not there yet.

## UI foundation (`ui/theme/`)

- `RostrumColors` names every palette role (bg, surface, raised,
  tonal/onTonal, border/borderStrong, the four text levels, accent/onAccent/
  accentText, success/warning/danger and their text variants, draftText,
  merge/onMerge). `DarkRostrumColors` is the only palette, provided through
  `LocalRostrumColors` and read as `RostrumTheme.colors`.
- `toMaterialColorScheme()` maps the palette onto a Material 3 dark scheme, so
  stock components match without per-call overrides. The error and tertiary
  container tints are derived: the status colour at 16% over `surface`.
- `RostrumFonts.Sans` (IBM Plex Sans 1.1.0) and `RostrumFonts.Mono`
  (JetBrains Mono 2.304), at weights 400, 500, and 600, are static TTFs in
  `res/font/`. Their OFL licenses are in `assets/licenses/`.
  `RostrumTypography` is Material's scale set in Plex Sans;
  `RostrumTheme.mono` gives `code`, `codeStrong`, and `number` styles.
- `RostrumText` (`ui/theme/TextStyles.kt`) names the mockups' recurring text
  styles (screen title, row title, section label, chip, mono sizes, diff
  lines).
- Packages: see `docs/features/android_app.md`; `.ui.theme` belongs here and
  `.data.RustCore` is the FFI probe.

## Common tasks

```sh
android/scripts/build-apk.sh              # release APK, end to end
android/scripts/publish-apk.sh            # hand it to rostrumd
cd android && ./gradlew :app:assembleDebug -Prostrum.cargoProfile=dev   # fast debug build
cd android && ./gradlew buildSrc:test     # ReleaseSigning / .env parser tests
cd android && ./gradlew :app:testDebugUnitTest -Prostrum.cargoProfile=dev   # app JVM unit tests (JUnit 5)
~/.cargo/bin/cargo test -p rostrum-ffi    # Rust side
```

- **Upgrading uniffi:** change the exact pin in the root `Cargo.toml`, run
  `cargo update -p uniffi`, and rebuild. `Cargo.lock` is an input of both
  tasks, so the library and the bindings are rebuilt together.
- **Upgrading the NDK:** install it, then change `ndk` in
  `gradle/libs.versions.toml`. Gradle, AGP's stripping, and `build-apk.sh` all
  read that one entry.
- **Adding an ABI:** add it to `androidAbis` in `app/build.gradle.kts` and its
  Rust target to `RUST_TARGETS` in `build-apk.sh`.

## Invariants

- **Pinned uniffi, one version end to end.** `uniffi = "=0.32.1"` is an exact
  pin, and the `uniffi-bindgen` that generates the Kotlin is built from the
  same locked version as the scaffolding compiled into the `.so`. Generated
  code is only valid against its own uniffi release. On mismatch the bindings'
  checksum check throws on first call.
- **The `.so` must keep its symbol table until bindgen has read it.**
  uniffi_bindgen 0.32 extracts metadata from `.symtab` (`elf.syms`), not
  `.dynsym`, so `strip = "symbols"` in the cargo profile breaks generation.
  Stripping happens in AGP at packaging.
- **`panic = "unwind"`** in any profile used for the `.so`. UniFFI catches
  panics at the boundary and raises them in Kotlin; `abort` would kill the
  process instead.
- **Names the build depends on:** cargo package `rostrum-ffi`, library
  `rostrum_ffi` (hence `librostrum_ffi.so` and Kotlin package
  `uniffi.rostrum_ffi`), and binary `uniffi-bindgen`. A replacement crate
  keeps all three, plus `uniffi.toml`.
- **JNA ≥ 5.12, as the AAR.** The generated bindings need it, and only the
  `@aar` artifact carries `libjnidispatch.so` for Android ABIs. It is pinned
  at 5.19.1, and R8 must keep `com.sun.jna.**` and `uniffi.**`.
- **The keystore stays outside the repository; secrets are never committed.**
  `android/.env`, `*.jks`, and `*.keystore` are gitignored.
- **The publish JSON is a contract.** It has exactly five keys, with
  `version_code` and `size` as JSON numbers. Both files are replaced
  atomically, the APK first.
- **Never `cargo` from PATH.** On the development machine, `cargo` on PATH is
  the rbs shim (`~/.local/share/rbs/shim/cargo`), which may run builds on a
  remote host that has no NDK. Gradle and both scripts use `$CARGO`, else
  `$CARGO_HOME/bin/cargo`, else `~/.cargo/bin/cargo` (the rustup proxy).
  cargo-ndk then inherits the real toolchain binary through the `CARGO`
  variable the outer cargo sets.
- **`--locked`.** Both cargo invocations refuse to modify `Cargo.lock`, so
  after changing dependencies, update the lockfile before building.
- **ABIs and API level.** `arm64-v8a` and `x86_64` must agree between
  `androidAbis` and the Rust targets. cargo-ndk links at minSdk (28).
- **compileSdk is the ceiling for AAR `minCompileSdk`.** Some newer AndroidX
  releases require 37, which is not a released platform, so the catalog pins
  sit below them (see the comments in `libs.versions.toml`).

## Files

| File | Role |
|---|---|
| `crates/rostrum-ffi/Cargo.toml` | Crate type, the `uniffi-bindgen` bin, per-target `cli` feature |
| `crates/rostrum-ffi/src/lib.rs` | `setup_scaffolding!`, placeholder `ffi_version()` |
| `crates/rostrum-ffi/src/bin/uniffi-bindgen.rs` | `uniffi_bindgen_main()` on the host, stub on Android |
| `crates/rostrum-ffi/uniffi.toml` | Kotlin binding config (`android = true`) |
| `Cargo.toml` (root) | `uniffi = "=0.32.1"`, `rostrum-ffi` workspace dep, `[profile.android-release]` |
| `android/settings.gradle.kts`, `android/build.gradle.kts` | Repositories, the `:app` module, plugin declarations |
| `android/gradle/libs.versions.toml` | Every pinned version, including `ndk` and the SDK levels |
| `android/gradle.properties` | Built-in Kotlin, AndroidX, `rostrum.cargoProfile` |
| `android/gradle/gradle-daemon-jvm.properties` | Daemon runs on JDK 21 |
| `android/gradle/wrapper/*`, `android/gradlew*` | Gradle 9.6.1 wrapper with distribution checksum |
| `android/buildSrc/src/main/kotlin/rostrum/buildlogic/CargoNdkBuild.kt` | `CargoNdkBuild` task |
| `android/buildSrc/src/main/kotlin/rostrum/buildlogic/UniffiBindgen.kt` | `UniffiBindgen` task |
| `android/buildSrc/src/main/kotlin/rostrum/buildlogic/Toolchain.kt` | `cargoExecutable()`, `ndkDirectory()` |
| `android/buildSrc/src/main/kotlin/rostrum/buildlogic/ReleaseSigning.kt` | `ReleaseSigning` (`Keystore` / `Unavailable`), `.env` parser |
| `android/buildSrc/src/test/kotlin/rostrum/buildlogic/ReleaseSigningTest.kt` | Parser and fallback tests |
| `android/app/build.gradle.kts` | Wires the Rust tasks and source dirs, `android {}`, signing, dependencies |
| `android/app/proguard-rules.pro` | R8 keep rules for JNA and `uniffi.**` |
| `android/app/src/main/AndroidManifest.xml` | Permissions, `MainActivity`, `rostrum://pair` filter |
| `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/MainActivity.kt` | Edge-to-edge single activity (see `android_app`) |
| `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/theme/Color.kt` | `RostrumColors`, `DarkRostrumColors`, `LocalRostrumColors` |
| `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/theme/Type.kt` | `RostrumFonts`, `RostrumTypography`, `RostrumMonoTypography` |
| `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/theme/Theme.kt` | `RostrumTheme` composable and accessors, `toMaterialColorScheme()` |
| `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/ui/theme/TextStyles.kt` | `RostrumText`: the mockups' text styles |
| `android/app/src/main/kotlin/io/github/rhizonymph/rostrum/data/RustCore.kt` | `CoreLink`, `RustCore.probe()` |
| `android/app/src/main/res/` | Window theme, strings, adaptive icon, fonts |
| `android/app/src/main/assets/licenses/` | OFL texts for both font families |
| `android/.env.example` | Signing settings template (`android/.env` is gitignored) |
| `android/scripts/build-apk.sh` | Preflight checks, then `:app:assembleRelease` |
| `android/scripts/publish-apk.sh` | Verify, then atomic publish of APK and JSON for `rostrumd` |
