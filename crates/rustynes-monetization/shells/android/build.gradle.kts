// build.gradle.kts (app module) — excerpt showing the monetization-relevant wiring.
//
// This is NOT a complete Android Gradle file; it highlights only the pieces this
// skeleton needs: the SDK dependencies, the BuildConfig fields the Kotlin reads, and
// the jniLibs source set where cargo-ndk drops librustynes_monetization.so.

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "app.rustynes"
    compileSdk = 35 // Play Billing 9 / current target requirement

    defaultConfig {
        applicationId = "app.rustynes"
        minSdk = 24          // AppLovin MAX 13 supports down to 21; 24 is a safe floor
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"

        // Values consumed by RustyNesApp.kt / AdGate.kt via BuildConfig.*.
        // Keep real keys out of source control — inject from gradle.properties or CI.
        buildConfigField("String", "APPLOVIN_SDK_KEY", "\"${providers.gradleProperty("applovinSdkKey").orNull ?: ""}\"")
        buildConfigField("String", "REVENUECAT_API_KEY", "\"${providers.gradleProperty("revenueCatGoogleKey").orNull ?: ""}\"")
        buildConfigField("String", "MAX_INTERSTITIAL_AD_UNIT_ID", "\"${providers.gradleProperty("maxInterstitialAdUnitId").orNull ?: ""}\"")
    }

    buildFeatures {
        buildConfig = true
    }

    buildTypes {
        debug {
            // Local-dev tester unlock: lets Billing.kt force premium WITHOUT a purchase on a
            // debug build, for QA. NEVER reaches Play — release sets this false, and the
            // closed-test track is a release build, so its testers must be unlocked via a
            // RevenueCat promotional grant or Google Play license testing (runbook §5a).
            buildConfigField("boolean", "TESTER_UNLOCK", "true")
        }
        release {
            buildConfigField("boolean", "TESTER_UNLOCK", "false")
            // If you enable R8/minify, add keep rules for the JNA + generated FFI classes.
        }
    }

    // cargo-ndk writes the per-ABI .so files here (see README build step).
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    // --- Monetization SDKs -----------------------------------------------------------
    // Pin to the newest patch before release; versions below are recent at time of writing.
    implementation("com.applovin:applovin-sdk:13.0.1")          // MAX mediation
    implementation("com.revenuecat.purchases:purchases:8.10.0") // RevenueCat (Google)

    // AppLovin's Google bidding/AdMob adapter (very common in a MAX waterfall) requires
    // the AdMob app id in the manifest (see AndroidManifest.xml). Add mediation adapters
    // here as you enable each network in the MAX dashboard, e.g.:
    // implementation("com.applovin.mediation:google-adapter:x.y.z")

    // --- UniFFI runtime --------------------------------------------------------------
    // The generated app/rustynes/ffi/rustynes_monetization.kt uses JNA to load the native .so.
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
}
