# --- JNA ----------------------------------------------------------------------
# JNA binds Java to native code by reflection (Structure fields, Callback
# methods, Native.register on classes by name) and its native dispatcher calls
# back into these classes by name, so none of them may be renamed or removed.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
# JNA's desktop-only paths reference AWT, which Android does not have.
-dontwarn java.awt.**

# --- UniFFI bindings ----------------------------------------------------------
# Generated into the `uniffi.<crate>` package (uniffi.rostrum_ffi). Its
# `UniffiLib` is registered with JNA by class, its structs and callback
# interfaces are reflected on, so the whole package is kept as generated.
-keep class uniffi.** { *; }

# --- Navigation typed routes --------------------------------------------------
# Navigation Compose resolves an enum route argument (`PrTab`) by its fully
# qualified class name at runtime, and builds every route's arguments from its
# kotlinx.serialization descriptor. Minified names break both on the first
# frame, so the route types keep their names and generated serializers.
-keep enum io.github.rhizonymph.rostrum.ui.navigation.** { *; }
-keep @kotlinx.serialization.Serializable class io.github.rhizonymph.rostrum.ui.navigation.** { *; }
-keepclassmembers class io.github.rhizonymph.rostrum.ui.navigation.** {
    *** Companion;
    kotlinx.serialization.KSerializer serializer(...);
}
-keep class io.github.rhizonymph.rostrum.ui.navigation.**$$serializer { *; }
