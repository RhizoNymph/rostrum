package rostrum.buildlogic

import org.gradle.api.DefaultTask
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.FileSystemOperations
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations
import javax.inject.Inject

/**
 * Generates the Kotlin bindings for a UniFFI library in library mode: the
 * interface is read out of the built `.so` itself (its `UNIFFI_META_*`
 * symbols), so the bindings describe exactly the binary that ships.
 *
 * The generator is the package's own `uniffi-bindgen` binary, built on the
 * host from the same locked `uniffi` version as the library.
 */
abstract class UniffiBindgen @Inject constructor(
    private val exec: ExecOperations,
    private val fs: FileSystemOperations,
) : DefaultTask() {
    @get:Input
    abstract val cargo: Property<String>

    /** Cargo package that owns the `uniffi-bindgen` binary. */
    @get:Input
    abstract val packageName: Property<String>

    @get:Internal
    abstract val workspaceDir: DirectoryProperty

    /**
     * A built Android `.so` of the library. Any ABI works; its symbol table
     * must be intact, which is why the cargo profile does not strip symbols.
     */
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val library: RegularFileProperty

    /** Other inputs of the generator: the lockfile (uniffi version) and `uniffi.toml`. */
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val configFiles: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val out = outputDir.get().asFile
        fs.delete { delete(out) }
        out.mkdirs()
        exec.exec {
            workingDir = workspaceDir.get().asFile
            executable = cargo.get()
            args(
                "run", "--locked", "-p", packageName.get(), "--bin", "uniffi-bindgen", "--",
                "generate",
                "--library", library.get().asFile.absolutePath,
                "--language", "kotlin",
                "--out-dir", out.absolutePath,
                // ktlint is not part of the toolchain; formatting is cosmetic.
                "--no-format",
            )
        }
    }
}
