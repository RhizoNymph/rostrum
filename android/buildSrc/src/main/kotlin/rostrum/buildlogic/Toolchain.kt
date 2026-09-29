package rostrum.buildlogic

import org.gradle.api.file.Directory
import org.gradle.api.provider.Provider
import org.gradle.api.provider.ProviderFactory
import java.io.StringReader
import java.util.Properties

/**
 * The cargo binary to run: `$CARGO`, else `$CARGO_HOME/bin/cargo`, else
 * `~/.cargo/bin/cargo` (the rustup proxy).
 *
 * Deliberately not a PATH lookup: on the development machine `cargo` on PATH
 * is a shim that may run the build on a remote host, which has no NDK.
 */
fun ProviderFactory.cargoExecutable(): Provider<String> =
    environmentVariable("CARGO")
        .orElse(environmentVariable("CARGO_HOME").map { "$it/bin/cargo" })
        .orElse(systemProperty("user.home").map { "$it/.cargo/bin/cargo" })

/**
 * The NDK root, first match wins:
 * 1. `$ANDROID_NDK_HOME`
 * 2. `ndk.dir` in `local.properties`
 * 3. `<sdk>/ndk/<ndkVersion>`, where `<sdk>` is `sdk.dir` in `local.properties`,
 *    else `$ANDROID_HOME`, else `$ANDROID_SDK_ROOT`.
 *
 * Only a candidate path: [CargoNdkBuild] checks that it holds an NDK, so a
 * missing one fails the build that needs it rather than every configuration.
 */
fun ProviderFactory.ndkDirectory(androidRoot: Directory, ndkVersion: String): Provider<String> {
    val local = localProperties(androidRoot)
    val sdk = local.map { it.getProperty("sdk.dir").orEmpty() }.filter { it.isNotEmpty() }
        .orElse(environmentVariable("ANDROID_HOME"))
        .orElse(environmentVariable("ANDROID_SDK_ROOT"))
    return environmentVariable("ANDROID_NDK_HOME")
        .orElse(local.map { it.getProperty("ndk.dir").orEmpty() }.filter { it.isNotEmpty() })
        .orElse(sdk.map { "$it/ndk/$ndkVersion" })
        .orElse("<no Android SDK: set sdk.dir in local.properties or ANDROID_HOME>")
}

private fun ProviderFactory.localProperties(androidRoot: Directory): Provider<Properties> =
    fileContents(androidRoot.file("local.properties")).asText
        .map { text -> Properties().apply { load(StringReader(text)) } }
        .orElse(Properties())
