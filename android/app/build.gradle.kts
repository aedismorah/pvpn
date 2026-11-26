plugins {
    id("com.android.application")
}

android {
    namespace = "com.pvpn.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.pvpn.app"
        minSdk = 29
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }
}

dependencies {
    implementation(files("libs/hev-socks5-tunnel.aar"))
}
