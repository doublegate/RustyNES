# v1.8.8 "Atlas" (Workstream J) — R8 full-mode keep rules.
#
# R8 full mode is default-on since AGP 8.0 and AGP 9.x adds
# `strictFullModeForKeepRules`: a bare `-keep class Foo` no longer implicitly
# keeps Foo's constructors or members — only the type itself. Anything reached
# by JNI/JNA/reflection (the entire UniFFI FFI surface, JNA's dispatcher, the
# Cast OptionsProvider) must therefore spell out `{ <init>(...); *; }` or R8
# strips the members and the *minified release* crashes at runtime even though
# the type name survives. These explicit keeps are what makes `assembleRelease`
# (isMinifyEnabled=true) safe — debug (unminified) never exercised them.
#
# Reference: Mozilla application-services' JNA consumer ruleset
# (https://github.com/mozilla/application-services/blob/main/proguard-rules-consumer-jna.pro)
# and developer.android.com/topic/performance/app-optimization/keep-rule-examples.

# --- Annotation / signature attributes ---------------------------------------
# JNA and UniFFI read generic signatures + (in)visible annotations reflectively
# to map Kotlin/Java types onto the C ABI. AGP 9.x tightened `-keepattributes`
# so wildcards no longer cover the RuntimeInvisible* families — list them all.
-keepattributes RuntimeVisibleAnnotations,RuntimeInvisibleAnnotations,RuntimeVisibleParameterAnnotations,RuntimeInvisibleParameterAnnotations,RuntimeVisibleTypeAnnotations,RuntimeInvisibleTypeAnnotations,AnnotationDefault,InnerClasses,EnclosingMethod,Signature,Exceptions

# --- JNA (the cdylib dispatcher UniFFI's bindings call through) ---------------
# Keep the whole JNA runtime + anything that subclasses a JNA type (Structure,
# Library, Callback, etc.) WITH its members and constructors — JNA instantiates
# and field-maps these via reflection at the native boundary.
-keep class com.sun.jna.** { *; }
-keep class * extends com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
# JNA references some optional AWT types on the JVM that don't exist on Android.
-dontwarn java.awt.**

# --- UniFFI generated bindings (the rustynes-mobile control surface) ----------
# The generated `uniffi.rustynes_mobile.*` package declares the JNA `Library`
# interface, the `UniffiLib`/`UniffiCleaner` plumbing, every record/enum data
# class, and the `uniffi_*` callback structures — all reached through JNA
# reflection. Keep the package WITH constructors + members (the strict-full-mode
# requirement). The nested generated classes are covered by `**`.
-keep class uniffi.rustynes_mobile.** { <init>(...); *; }
-keep interface uniffi.rustynes_mobile.** { *; }

# --- rustynes-android JNI seam (NativeRenderer) -------------------------------
# The wgpu SurfaceView render path resolves `native` methods on this class by
# name from JNI (`RegisterNatives`/by-signature). Keep the class holding the
# native methods + the native methods themselves so R8 can't rename/remove them.
-keepclasseswithmembernames class com.doublegate.rustynes.NativeRenderer { native <methods>; }
-keep class com.doublegate.rustynes.NativeRenderer { *; }

# --- Cast Application Framework OptionsProvider -------------------------------
# Declared in the manifest (OPTIONS_PROVIDER_CLASS_NAME) and instantiated by the
# Cast SDK via reflection. It is only touched when CastContext initializes
# (behind the default-off CHROMECAST_ENABLED flag), but R8 has no way to know
# that, so keep it + its no-arg constructor unconditionally.
-keep class com.doublegate.rustynes.RustyNesCastOptionsProvider { <init>(...); *; }
-keep class * implements com.google.android.gms.cast.framework.OptionsProvider { <init>(...); *; }

# --- Play Games Services v2 (Workstreams D+E) + Play Integrity / Core (L) ------
# The Play services / Play Core libraries ship their own consumer ProGuard rules in
# their AARs (so most of this is belt-and-suspenders under R8 strict-full-mode), but
# pin the public client surfaces RustyNES reflects/Task-chains through (PlayGames
# sign-in/achievements/leaderboards/Snapshots, the Integrity StandardIntegrity*
# builders, the AppUpdate/Review managers) so a strict pass can't strip a builder or
# a Task callback type. These are all DORMANT in the default build (PGS_ENABLED /
# PLAY_INTEGRITY_ENABLED / PLAY_BUILD are false), but R8 has no way to know that.
-keep class com.google.android.gms.games.** { *; }
-keep interface com.google.android.gms.games.** { *; }
-keep class com.google.android.play.core.integrity.** { *; }
-keep class com.google.android.play.core.appupdate.** { *; }
-keep class com.google.android.play.core.review.** { *; }
-keep class com.google.android.play.core.install.** { *; }
-dontwarn com.google.android.play.core.**

# --- ProfileInstaller -------------------------------------------------------
# The Baseline Profile installer ships a manifest ContentProvider + receiver it
# resolves reflectively; AGP usually keeps these, but pin them so a strict-full
# pass can't strip the on-device profile installation path.
-keep class androidx.profileinstaller.** { *; }

# --- Monetization UniFFI bindings (rustynes-monetization AdPolicy core) --------
# v2.0.3 "Harbor" (ADR 0025). The SECOND UniFFI crate's Kotlin bindings land in
# `com.doublegate.rustynes.monetization.ffi.*` (its own package via the crate's
# uniffi.toml, distinct from `uniffi.rustynes_mobile.*` above). Like the mobile
# bindings they are reached through JNA reflection, so keep the package WITH
# constructors + members — R8 strict-full-mode strips members otherwise and the
# minified `assemblePlayRelease` crashes at the first FFI call (AdPolicy / PlayProgress
# / the enums). Present only in the `play` variant; harmless (no-op keep) in `foss`.
-keep class com.doublegate.rustynes.monetization.ffi.** { <init>(...); *; }
-keep interface com.doublegate.rustynes.monetization.ffi.** { *; }

# --- AppLovin MAX (play-flavor ad mediation) ----------------------------------
# v2.0.3 "Harbor" (ADR 0025). AppLovin ships its own consumer rules in the AAR, but
# under AGP 9.x strict-full-mode pin the SDK + every mediation adapter (loaded by
# name/reflection from the MAX waterfall) WITH members so R8 can't rename/strip an
# adapter entry point or an ad listener the SDK calls back reflectively. `-dontwarn`
# covers the optional networks not on the classpath. `play`-only classes; the keeps
# are no-ops in the `foss` release (nothing under com.applovin is linked there).
-keep class com.applovin.** { *; }
-keep public class com.applovin.mediation.adapters.** { *; }
-keep public class * extends com.applovin.mediation.adapter.MaxAdapter { *; }
-dontwarn com.applovin.**

# --- RevenueCat (play-flavor entitlement source of truth) ---------------------
# v2.0.3 "Harbor" (ADR 0025). RevenueCat's Purchases SDK (RcBilling) uses reflection /
# serialization (kotlinx-serialization models, the BillingClient bridge, the
# UpdatedCustomerInfoListener). It ships consumer rules, but pin the public surface so a
# strict pass can't strip a model or the entitlement listener types RcBilling chains.
-keep class com.revenuecat.purchases.** { *; }
-dontwarn com.revenuecat.purchases.**
