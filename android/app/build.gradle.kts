import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

// Google's SDKs (the Wearable API for the watch link, ML Kit for the card scanner) are in the
// standard build only. F-Droid's may carry no Google artifact, and the offline build leaves them
// out too, since Play services would reach the network for them from its own process.
// src/google holds the code that calls them; src/nogoogle has the same names doing without.
val googleSdks = providers.gradleProperty("wmpass.offline").orNull != "true" &&
    providers.gradleProperty("wmpass.fdroid").orNull != "true"
val wearLink = googleSdks

// App ids for the cloud services backups can sign in to, registered for WM Pass in each
// service's console. Not secrets: the sign-in is PKCE. Missing means that service is not offered.
val localProperties = Properties().apply {
    rootProject.file("local.properties").takeIf { it.exists() }?.inputStream()?.use { load(it) }
}

fun appKey(name: String, env: String): String =
    providers.gradleProperty(name).orNull ?: localProperties.getProperty(name) ?: providers.environmentVariable(env).orNull ?: ""

val driveClientId = appKey("wmpass.driveClientId", "WMPASS_DRIVE_CLIENT_ID")

// The Play upload key, read the same way and never kept in the repo. Without it the release
// build comes out unsigned, which is what CI and F-Droid's builder (which signs its own) want.
val releaseStoreFile = appKey("wmpass.release.storeFile", "WMPASS_RELEASE_STORE_FILE")

android {
    namespace = "com.wasimaster.wmpass"
    compileSdk {
        version = release(37)
    }
    // The NDK that strips the Rust library and takes its symbols out for Play. Without the
    // version AGP looks for, it packages the library unstripped and the bundle carries no symbols.
    ndkVersion = "27.3.13750724"
    defaultConfig {
        applicationId = "com.wasimaster.wmpass"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        // Only for measuring the card readers on a phone (CardCorpusTest); run with am instrument.
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        buildConfigField("boolean", "WATCH", wearLink.toString())
        buildConfigField("boolean", "GOOGLE_SDKS", googleSdks.toString())
        buildConfigField("boolean", "OFFLINE", (providers.gradleProperty("wmpass.offline").orNull == "true").toString())
        buildConfigField("String", "DROPBOX_APP_KEY", "\"${appKey("wmpass.dropboxAppKey", "WMPASS_DROPBOX_APP_KEY")}\"")
        buildConfigField("String", "ONEDRIVE_CLIENT_ID", "\"${appKey("wmpass.oneDriveClientId", "WMPASS_ONEDRIVE_CLIENT_ID")}\"")
        buildConfigField("String", "BOX_CLIENT_ID", "\"${appKey("wmpass.boxClientId", "WMPASS_BOX_CLIENT_ID")}\"")
        buildConfigField("String", "BOX_CLIENT_SECRET", "\"${appKey("wmpass.boxClientSecret", "WMPASS_BOX_CLIENT_SECRET")}\"")
        buildConfigField("String", "DRIVE_CLIENT_ID", "\"$driveClientId\"")
        // Google sends a sign-in back on the reversed client id. Without one, a scheme nothing uses.
        manifestPlaceholders["driveScheme"] = driveClientId.removeSuffix(".apps.googleusercontent.com")
            .let { if (it.isEmpty()) "wmpass-no-drive" else "com.googleusercontent.apps.$it" }
        // Native symbols go into the bundle's metadata, where Play finds them for crash reports.
        // What gets installed is stripped all the same.
        ndk { debugSymbolLevel = "FULL" }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    signingConfigs {
        create("release") {
            if (releaseStoreFile.isNotEmpty()) {
                storeFile = rootProject.file(releaseStoreFile)
                storePassword = appKey("wmpass.release.storePassword", "WMPASS_RELEASE_STORE_PASSWORD")
                keyAlias = appKey("wmpass.release.keyAlias", "WMPASS_RELEASE_KEY_ALIAS")
                keyPassword = appKey("wmpass.release.keyPassword", "WMPASS_RELEASE_KEY_PASSWORD")
            }
        }
    }
    buildTypes {
        release {
            // One line, with findByName: F-Droid's builder deletes the signingConfigs block and
            // every signingConfig line, and the build must still come out whole and unsigned.
            signingConfig = signingConfigs.findByName("release")?.takeIf { it.storeFile?.exists() == true }
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    packaging {
        resources.excludes += setOf("META-INF/{AL2.0,LGPL2.1}", "META-INF/LICENSE*", "META-INF/versions/9/OSGI-INF/MANIFEST.MF")
        jniLibs.useLegacyPackaging = false
    }
    androidResources { ignoreAssetsPatterns += "*.md" }
    // Each app's export guide from the website, shown on the import screen.
    sourceSets["main"].assets.directories.add("../../docs/src/content/docs/guides/import")
    sourceSets["main"].assets.directories.add("../../data/brands")
    // Sites that share one account, for the editor's website and app suggestions.
    sourceSets["main"].assets.directories.add("../../data/related")
    // The in-app guide, written by scripts/build-help.mjs from help/ at the root.
    sourceSets["main"].assets.directories.add("../../data/help")
    sourceSets["main"].assets.directories.add("../../data/passkeys")
    // The card scanner's digit detector and its license.
    sourceSets["main"].assets.directories.add("../../data/cardscan")
    // The offline build drops the permission, so no line of code can reach a network.
    if (providers.gradleProperty("wmpass.offline").orNull == "true") {
        sourceSets["main"].manifest.srcFile("src/offline/AndroidManifest.xml")
    }
    testOptions.unitTests.isIncludeAndroidResources = true
    testOptions.unitTests.all {
        it.systemProperty("java.library.path", rootProject.file("../target/debug").absolutePath)
        it.systemProperty("roborazzi.test.record", "true")
        it.maxHeapSize = "3g"
    }
}

androidComponents {
    onVariants { variant ->
        variant.sources.kotlin?.addStaticSourceDirectory(if (googleSdks) "src/google/java" else "src/nogoogle/java")
    }
}

// Stills and JVM tests run against this computer's build of the native library.
tasks.withType<Test>().configureEach { dependsOn(":core:buildHostNative") }

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(project(":core"))
    // The watch link brings in Google's Wearable API, which the F-Droid and offline builds leave out.
    if (wearLink) implementation(project(":wearlink"))
    // The card scanner's text reader, from Play services, which fetches its model: about 260 KB here.
    if (googleSdks) implementation("com.google.android.gms:play-services-mlkit-text-recognition:19.0.1")
    implementation("com.wasimaster.wmsuite:theme")
    implementation("com.wasimaster.wmsuite:ui-core")
    implementation("com.wasimaster.wmsuite:settings-ui")
    implementation("com.wasimaster.wmsuite:sinks")

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation("androidx.lifecycle:lifecycle-process:${libs.versions.lifecycle.get()}")
    implementation(libs.androidx.biometric)
    implementation(libs.androidx.appcompat)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.androidx.autofill)
    implementation("androidx.credentials:credentials:1.6.0")
    implementation("androidx.glance:glance-appwidget:1.2.0")
    implementation("androidx.glance:glance-material3:1.2.0")
    implementation(libs.androidx.camera.core)
    implementation(libs.androidx.camera.camera2)
    implementation(libs.androidx.camera.lifecycle)
    implementation(libs.androidx.camera.view)
    implementation(libs.zxing.core)
    // Saved Wi-Fi passwords, read through Shizuku only when asked. Both Apache-2.0.
    implementation("dev.rikka.shizuku:api:13.1.5")
    implementation("dev.rikka.shizuku:provider:13.1.5")
    implementation("org.lsposed.hiddenapibypass:hiddenapibypass:6.2")
    implementation("androidx.work:work-runtime-ktx:2.12.0")

    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.junit)
    androidTestImplementation(libs.androidx.test.runner)
    testImplementation(libs.robolectric)
    testImplementation(libs.roborazzi)
    testImplementation(libs.roborazzi.compose)
    testImplementation(platform(libs.androidx.compose.bom))
    testImplementation(libs.androidx.compose.ui.test.junit4)
    debugImplementation(libs.androidx.compose.ui.test.manifest)
}
