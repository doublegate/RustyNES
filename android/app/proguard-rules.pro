# UniFFI's generated Kotlin loads the native library through JNA reflection;
# keep the dispatcher + the generated binding package so R8 minification (enabled
# in release) doesn't strip the FFI surface.
-keep class com.sun.jna.** { *; }
-keep class uniffi.rustynes_mobile.** { *; }
-dontwarn java.awt.**
