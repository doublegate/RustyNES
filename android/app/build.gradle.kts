import java.io.FileInputStream
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// The pure-Rust workspace lives one directory up from this Gradle module.
val workspaceRoot: File = rootDir.parentFile
// Where `cargo-ndk` writes the per-ABI `.so` files and where UniFFI writes the
// generated Kotlin bindings (added to the main source set below).
val jniLibsDir: File = file("src/main/jniLibs")
val uniffiGenDir: File = layout.buildDirectory.dir("generated/uniffi").get().asFile

// ABIs cross-compiled by cargo-ndk (the jniLibs always contain both): arm64 for
// devices, x86_64 for the emulator / CI smoke test. The *release* variant then
// packages arm64 only (smallest AAB — see the `release` buildType's abiFilters);
// the *debug* variant keeps x86_64 so it runs on the emulator. armeabi-v7a is
// opt-in (see the maintainer defaults).
val builtAbis = listOf("arm64-v8a", "x86_64")
val shipAbi = "arm64-v8a"

android {
    namespace = "com.doublegate.rustynes"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.doublegate.rustynes"
        minSdk = 26 // AAudio floor.
        targetSdk = 35 // Play mandate since 2025-08-31.
        versionCode = 10801 // 1.8.1
        versionName = "1.8.1"
        // No abiFilters here — set per buildType so release ships arm64 only
        // while debug keeps x86_64 for the emulator.
    }

    // Release signing reads `keystore.properties` (gitignored) or env vars; when
    // neither is present the release build stays unsigned so CI `bundleRelease`
    // still links and verifies. Play App Signing manages the app key; this is the
    // upload key only.
    val keystorePropsFile = rootProject.file("keystore.properties")
    signingConfigs {
        create("upload") {
            if (keystorePropsFile.exists()) {
                val props = Properties().apply { load(FileInputStream(keystorePropsFile)) }
                storeFile = file(props.getProperty("storeFile"))
                storePassword = props.getProperty("storePassword")
                keyAlias = props.getProperty("keyAlias")
                keyPassword = props.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // The shipped AAB carries arm64 only (smallest download).
            ndk { abiFilters += shipAbi }
            if (keystorePropsFile.exists()) {
                signingConfig = signingConfigs.getByName("upload")
            }
        }
        debug {
            applicationIdSuffix = ".debug"
            // Debug keeps x86_64 too so it installs on the emulator / CI.
            ndk { abiFilters += builtAbis }
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true // exposes BuildConfig.DEBUG for the debug-only ROM autoload.
    }

    sourceSets["main"].java.srcDir(uniffiGenDir)
    sourceSets["main"].jniLibs.srcDir(jniLibsDir)

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    // 16 KB page alignment (Play requirement for Android 15+). NDK r27+ aligns
    // by default; AGP packages the aligned `.so` unchanged.
    packaging {
        jniLibs { useLegacyPackaging = false }
    }
}

// --- Rust integration -------------------------------------------------------

// Build the workspace `.so` libraries for every shipped ABI via cargo-ndk and
// drop them into `jniLibs/<abi>/`. Requires the Android Rust targets + cargo-ndk
// (`rustup target add aarch64-linux-android x86_64-linux-android;
//   cargo install cargo-ndk`) and ANDROID_NDK_HOME (or an SDK-resolved NDK).
val cargoNdkBuild by tasks.registering(Exec::class) {
    group = "rust"
    description = "Cross-compile rustynes-mobile + rustynes-android into jniLibs via cargo-ndk."
    workingDir = workspaceRoot
    val abiArgs = builtAbis.flatMap { listOf("-t", it) }
    commandLine(
        listOf("cargo", "ndk") + abiArgs +
            listOf(
                "-o", jniLibsDir.absolutePath,
                "--platform", "26",
                "build", "--release",
                "-p", "rustynes-mobile",
                "-p", "rustynes-android",
            ),
    )
}

// Generate the Kotlin bindings from the compiled arm64 cdylib (the UniFFI API is
// target-independent, so any built library serves as the source of truth).
val uniffiBindgen by tasks.registering(Exec::class) {
    group = "rust"
    description = "Generate Kotlin bindings for the rustynes-mobile control surface via UniFFI."
    dependsOn(cargoNdkBuild)
    workingDir = workspaceRoot
    val lib = workspaceRoot.resolve("target/aarch64-linux-android/release/librustynes_mobile.so")
    commandLine(
        "cargo", "run", "-q", "-p", "rustynes-mobile", "--bin", "uniffi-bindgen", "--",
        "generate", "--library", lib.absolutePath,
        "--language", "kotlin", "--out-dir", uniffiGenDir.absolutePath,
    )
}

tasks.named("preBuild") { dependsOn(uniffiBindgen) }

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    // UniFFI's generated Kotlin loads the cdylib through JNA; the `@aar`
    // classifier pulls the Android-native JNA dispatcher.
    implementation("net.java.dev.jna:jna:5.15.0@aar")
    // Play Billing — the one-time "Full Unlock" IAP (Workstream M, freemium model).
    implementation("com.android.billingclient:billing-ktx:8.0.0")
}
