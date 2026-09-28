// Root build: declares every plugin once so module scripts only apply them.
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.compose) apply false
}
