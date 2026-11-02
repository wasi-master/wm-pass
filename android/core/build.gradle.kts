import javax.inject.Inject
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.process.ExecOperations

plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.serialization)
}

android {
    namespace = "com.wasimaster.wmpass.core"
    compileSdk {
        version = release(37)
    }
    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    testOptions.unitTests.isIncludeAndroidResources = true
    testOptions.unitTests.all {
        // The native library for the machine running the tests, so a unit test goes
        // through the real Rust core rather than a fake of it.
        it.systemProperty("java.library.path", rootProject.file("../target/debug").absolutePath)
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

// Built from source on every machine: a binary in a password manager's tree could not
// be audited. `-Pwmpass.abi=arm64-v8a` builds one ABI, for a quick install on a phone.
val wantedAbis = (findProperty("wmpass.abi") as String?)?.split(",")
    ?: listOf("arm64-v8a", "armeabi-v7a", "x86_64")
val cargoRoot = rootProject.file("..")

abstract class CargoNdk @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:OutputDirectory abstract val out: DirectoryProperty
    @get:Input abstract val abis: ListProperty<String>
    @get:InputDirectory @get:PathSensitive(PathSensitivity.RELATIVE) abstract val crates: DirectoryProperty
    @get:InputFile @get:PathSensitive(PathSensitivity.RELATIVE) abstract val lock: RegularFileProperty
    @get:Internal abstract val workspace: DirectoryProperty
    @get:Internal abstract val sdk: DirectoryProperty

    @TaskAction
    fun build() {
        val ndk = System.getenv("ANDROID_NDK") ?: System.getenv("ANDROID_NDK_HOME")
            ?: sdk.get().asFile.resolve("ndk").listFiles()?.maxOrNull()?.path
            ?: error("Set ANDROID_NDK, or install an NDK under the Android SDK")
        exec.exec {
            workingDir = workspace.get().asFile
            environment("ANDROID_NDK_HOME", ndk)
            val args = mutableListOf("cargo", "ndk")
            abis.get().forEach { args += listOf("-t", it) }
            args += listOf("-o", out.get().asFile.path, "build", "--release", "-p", "wmpass-jni")
            commandLine(args)
        }
    }
}

val buildNative = tasks.register<CargoNdk>("buildNative") {
    out.set(layout.buildDirectory.dir("jniLibs"))
    this.abis.set(wantedAbis)
    crates.set(cargoRoot.resolve("crates"))
    lock.set(cargoRoot.resolve("Cargo.lock"))
    workspace.set(cargoRoot)
    sdk.set(androidComponents.sdkComponents.sdkDirectory)
}

// The same library for this computer, loaded by unit tests.
val buildHostNative by tasks.registering(Exec::class) {
    workingDir = cargoRoot
    commandLine("cargo", "build", "-p", "wmpass-jni")
    inputs.dir(cargoRoot.resolve("crates"))
    outputs.dir(cargoRoot.resolve("target/debug"))
}

androidComponents {
    onVariants { variant ->
        variant.sources.jniLibs?.addGeneratedSourceDirectory(buildNative, CargoNdk::out)
    }
}
tasks.withType<Test>().configureEach { dependsOn(buildHostNative) }

dependencies {
    api(libs.kotlinx.coroutines.android)
    api(libs.kotlinx.serialization.json)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.biometric)
    implementation(platform(libs.androidx.compose.bom))
    implementation("androidx.compose.runtime:runtime")
    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
}
