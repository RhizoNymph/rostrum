package rostrum.buildlogic

import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.FileSystemOperations
import org.gradle.api.provider.ListProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations
import java.io.File
import javax.inject.Inject

/**
 * Cross-compiles one cargo package's `cdylib` for each Android ABI with
 * cargo-ndk, leaving `<outputDir>/<abi>/lib<name>.so` — the layout AGP expects
 * of a jniLibs directory.
 *
 * Up-to-date checks are Gradle's (the workspace manifests and every file under
 * `crates/`); when they fire, cargo's own incremental build does the rest.
 */
abstract class CargoNdkBuild @Inject constructor(
    private val exec: ExecOperations,
    private val fs: FileSystemOperations,
) : DefaultTask() {
    /** The cargo binary. See [cargoExecutable] for why this is not a PATH lookup. */
    @get:Input
    abstract val cargo: Property<String>

    /** NDK root handed to cargo-ndk. A different NDK is a different build. */
    @get:Input
    abstract val ndkDir: Property<String>

    /** Cargo package to build, e.g. `rostrum-ffi`. */
    @get:Input
    abstract val packageName: Property<String>

    /** Cargo profile, e.g. `android-release`. */
    @get:Input
    abstract val profile: Property<String>

    /** Android ABI names (`arm64-v8a`, `x86_64`, ...), as cargo-ndk spells them. */
    @get:Input
    abstract val abis: ListProperty<String>

    /** Android API level the library links against; the app's minSdk. */
    @get:Input
    abstract val apiLevel: Property<Int>

    /** Cargo workspace root, where cargo runs. */
    @get:Internal
    abstract val workspaceDir: DirectoryProperty

    /** Everything whose change can change the built library. */
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val sources: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun build() {
        val ndk = File(ndkDir.get())
        if (!ndk.resolve("toolchains/llvm/prebuilt").isDirectory) {
            throw GradleException(
                "No Android NDK at '$ndk'. Install the NDK pinned as `ndk` in gradle/libs.versions.toml " +
                    "into <sdk>/ndk/<version>, or point ANDROID_NDK_HOME (or ndk.dir in local.properties) " +
                    "at one. See docs/features/android_build.md.",
            )
        }
        val out = outputDir.get().asFile
        // Stale ABIs or renamed libraries must not survive into the APK.
        fs.delete { delete(out) }
        out.mkdirs()

        val args = buildList {
            add("ndk")
            abis.get().forEach { abi -> add("-t"); add(abi) }
            addAll(listOf("-P", apiLevel.get().toString(), "-o", out.absolutePath))
            addAll(listOf("build", "--lib", "--locked", "--profile", profile.get(), "-p", packageName.get()))
        }
        exec.exec {
            workingDir = workspaceDir.get().asFile
            executable = cargo.get()
            args(args)
            environment("ANDROID_NDK_HOME", ndk.absolutePath)
        }
    }
}
