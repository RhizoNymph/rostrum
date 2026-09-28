// Build logic for the Rust half of the app: the cargo-ndk and uniffi-bindgen
// tasks, NDK/cargo discovery, and release-signing credentials. Plain Gradle API
// only, so it does not depend on (or pin) AGP.
//
// Tests are not run by the app build; run them with `./gradlew buildSrc:test`
// (from android/, so the daemon is the JDK 21 one the root build asks for).
plugins {
    `kotlin-dsl`
}

dependencies {
    testImplementation(platform(libs.junit.bom))
    testImplementation(libs.junit.jupiter)
    testRuntimeOnly(libs.junit.platform.launcher)
}

tasks.test {
    useJUnitPlatform()
}
