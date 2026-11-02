pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}
plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "wm-pass-android"

// The WM Suite libraries come from source, so F-Droid and CI need nothing but this
// checkout. `com.wasimaster.wmsuite:<module>` resolves to vendor/wm-suite.
includeBuild("vendor/wm-suite")

include(":core")
include(":app")
include(":wear")
include(":wearlink")
