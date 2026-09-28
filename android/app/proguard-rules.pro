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
