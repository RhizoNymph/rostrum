dependencyResolutionManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
    // The app's catalog, so build logic pins its versions in the same place.
    versionCatalogs {
        create("libs") {
            from(files("../gradle/libs.versions.toml"))
        }
    }
}

rootProject.name = "buildSrc"
