plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.agb.duo"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.agb.duo"
        minSdk = 30
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += "arm64-v8a" }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    // .agbw files are zips already, and are read as streams.
    androidResources { noCompress += "agbw" }
    // The engine's command-line program is packed as a library (libagbcli.so) and run as a
    // program: for that it has to be unpacked on the device.
    packaging { jniLibs { useLegacyPackaging = true } }
    buildTypes {
        release {
            isMinifyEnabled = false
            // No release key in the repository: `assembleRelease` is signed like a debug build.
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}
