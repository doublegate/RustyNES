import java.io.FileInputStream
import java.util.Properties

plugins {
    id("com.android.application")
    // v1.8.8 "Atlas": AGP 9 supplies Kotlin itself (built-in Kotlin), so the
    // standalone `org.jetbrains.kotlin.android` plugin is no longer applied. The
    // Compose compiler plugin remains a separate, explicitly versioned plugin.
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
    // v1.8.8 "Atlas" (Workstream A): Android 16 QPR (API 37). The Play mandate
    // requires new submissions/updates to target Android 16 (API 36) after
    // 2026-08-31; we compile against the newer 37 SDK (AGP 9.2 supports it) so the
    // latest AndroidX (core 1.19 / lifecycle 2.11, which require compileSdk 37) link.
    compileSdk = 37

    defaultConfig {
        applicationId = "com.doublegate.rustynes"
        minSdk = 26 // AAudio floor.
        targetSdk = 36 // Play mandate (Android 16) from 2026-08-31.
        versionCode = 10808 // 1.8.8
        versionName = "1.8.8"
        // No abiFilters here — set per buildType so release ships arm64 only
        // while debug keeps x86_64 for the emulator.
        // PLAY_BUILD gates the freemium (demo timer + persistence locks + Billing).
        // Default false → sideload/GitHub/dev builds are full-featured; the Google
        // Play AAB sets it true (v1.8.8 "Atlas" launch — postponed from v1.8.6). See
        // the v1.8.0 plan's monetization timing.
        buildConfigField("boolean", "PLAY_BUILD", "false")
        // CHROMECAST_ENABLED gates the experimental Cast Application Framework
        // (CAF) sender path (v1.8.7, #38) — a ~20-30fps SPECTATOR mirror to a
        // custom Web Receiver, distinct from the primary low-latency Presentation
        // API cast (Cast.kt, which is always available). Default false: no Cast
        // button, no CastContext init, zero behavior change. It stays off until the
        // maintainer does the deferred ops (a $5 Cast Developer Console account, a
        // registered Receiver App ID, and HTTPS hosting of android/cast-receiver/).
        // See android/cast-receiver/README.md + ChromecastSender.kt.
        buildConfigField("boolean", "CHROMECAST_ENABLED", "false")
    }

    // Release signing reads `keystore.properties` (gitignored) or env vars; when
    // neither is present the release build stays unsigned so CI `bundleRelease`
    // still links and verifies. Play App Signing manages the app key; this is the
    // upload key only.
    val keystorePropsFile = rootProject.file("keystore.properties")
    // CI / automated signing: the same four values via env vars (e.g. GitHub Actions
    // secrets) when the gitignored file isn't present. `RUSTYNES_UPLOAD_STORE_FILE`
    // gates it; the others are read only when it is set.
    val keystoreEnvFile = System.getenv("RUSTYNES_UPLOAD_STORE_FILE")
    signingConfigs {
        create("upload") {
            if (keystorePropsFile.exists()) {
                val props = Properties().apply { load(FileInputStream(keystorePropsFile)) }
                storeFile = file(props.getProperty("storeFile"))
                storePassword = props.getProperty("storePassword")
                keyAlias = props.getProperty("keyAlias")
                keyPassword = props.getProperty("keyPassword")
            } else if (keystoreEnvFile != null) {
                storeFile = file(keystoreEnvFile)
                storePassword = System.getenv("RUSTYNES_UPLOAD_STORE_PASSWORD")
                keyAlias = System.getenv("RUSTYNES_UPLOAD_KEY_ALIAS")
                keyPassword = System.getenv("RUSTYNES_UPLOAD_KEY_PASSWORD")
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
            if (keystorePropsFile.exists() || keystoreEnvFile != null) {
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

    // v1.8.8 "Atlas" (Workstream B): auto-generate the per-app `locales_config.xml`
    // from the `values-*` resource folders and reference it in the merged manifest
    // (so the system Settings -> Apps -> RustyNES -> Language entry appears on
    // Android 13+). AGP reads `res/resources.properties` (unqualifiedResLocale=en) to
    // know the default `values/` locale, and the `values-es/` folder to add Spanish.
    // The in-app picker (AppCompatDelegate.setApplicationLocales) back-ports the same
    // selection to API 24+ via androidx.appcompat below.
    androidResources {
        generateLocaleConfig = true
    }

    // The UniFFI-generated bindings are Kotlin (.kt). Under AGP 9's built-in Kotlin,
    // generated Kotlin sources must be registered on the source set's `kotlin`
    // directories — adding them to `.java` (the pre-AGP-9 way) no longer feeds the
    // Kotlin compiler, so the binding types (NesController, NpStatus, …) go
    // unresolved. See AGP 9 built-in-Kotlin migration notes.
    sourceSets["main"].kotlin.srcDir(uniffiGenDir)
    sourceSets["main"].jniLibs.srcDir(jniLibsDir)

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // 16 KB page alignment (Play requirement for Android 15+). NDK r27+ aligns
    // by default; AGP packages the aligned `.so` unchanged.
    packaging {
        jniLibs { useLegacyPackaging = false }
    }
}

// Built-in Kotlin (AGP 9): the old `android.kotlinOptions { jvmTarget }` DSL moved
// to the top-level `kotlin.compilerOptions` block. The target defaults to
// `compileOptions.targetCompatibility` (17 above); set it explicitly for clarity.
kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
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
    // v1.8.8 "Atlas": Compose BOM 2025.09.01 (material3 1.4.0 — the stable M3 set
    // for the launch; M3 Expressive's spring-physics/wavy components live in the
    // 1.5.0-alpha line and are deliberately not pulled in here). Bumped from
    // 2024.12.01 so the adaptive APIs (Window Size Classes, ListDetailPaneScaffold)
    // and a current Compose runtime are available.
    val composeBom = platform("androidx.compose:compose-bom:2026.06.00")
    implementation(composeBom)
    implementation("androidx.core:core-ktx:1.19.0")
    // v1.8.8 "Atlas" (Workstream B): per-app language. AppCompat 1.6.0+ supplies the
    // back-compat AppCompatDelegate.setApplicationLocales (delegating to the platform
    // LocaleManager on API 33+, and a manual override on API 24..32). MainActivity must
    // extend AppCompatActivity and the launch theme must derive from an AppCompat theme
    // for the locale APIs to take effect under Compose.
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.11.0")
    // collectAsStateWithLifecycle for the controller-connect StateFlow (v1.8.7, #41).
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.11.0")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    // v1.8.8 "Atlas" (Workstream A): adaptive layouts. `adaptive` carries
    // currentWindowAdaptiveInfo()/WindowSizeClass (the single layout driver);
    // -layout carries ListDetailPaneScaffold; -navigation carries the predictive-
    // back-aware NavigableListDetailPaneScaffold for the expanded two-pane.
    implementation("androidx.compose.material3.adaptive:adaptive:1.2.0")
    implementation("androidx.compose.material3.adaptive:adaptive-layout:1.2.0")
    implementation("androidx.compose.material3.adaptive:adaptive-navigation:1.2.0")
    // WindowInfoTracker / FoldingFeature for foldable-posture awareness.
    implementation("androidx.window:window:1.5.1")
    // v1.8.8 "Atlas" (Workstream K): Android-12+ system splash via the back-compat
    // SplashScreen API (installSplashScreen() before super.onCreate()).
    implementation("androidx.core:core-splashscreen:1.2.0")
    // v1.8.8 "Atlas" (Workstream C): async image loading + caching for the box-art
    // library grid. Coil 3 (io.coil-kt.coil3) loads local `content://`/`file://` URIs
    // through Android's ContentResolver WITHOUT any network artifact, so we add only
    // `coil-compose` (the core + the AsyncImage composable). The libretro box-art
    // auto-match (BoxArt.kt) does its own one-shot HttpURLConnection download to a
    // file:// cache, so no Coil network fetcher (coil-network-*) is pulled in either.
    implementation("io.coil-kt.coil3:coil-compose:3.5.0")
    // UniFFI's generated Kotlin loads the cdylib through JNA; the `@aar`
    // classifier pulls the Android-native JNA dispatcher.
    implementation("net.java.dev.jna:jna:5.18.1@aar")
    // Play Billing — the one-time "Full Unlock" IAP (Workstream M, freemium model).
    // Pinned at 8.0.0 here: Billing 9.x is an API-breaking major (the v1.8.8 Play
    // launch / Workstream P revisits the entitlement code), and this Atlas-foundation
    // pass is presentation/Gradle only — bumping it would touch Billing.kt/LicenseManager.
    implementation("com.android.billingclient:billing-ktx:8.0.0")
    // Cast Application Framework sender (v1.8.7, #38). Linked but DORMANT: it does
    // nothing until CastContext is initialized, which only happens behind the
    // default-off BuildConfig.CHROMECAST_ENABLED flag (see ChromecastSender.kt).
    implementation("com.google.android.gms:play-services-cast-framework:22.1.0")
}
