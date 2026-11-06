plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "sh.fieldsmith.app"
    compileSdk = 35
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "sh.fieldsmith.app"
        minSdk = 26
        targetSdk = 35
        versionCode = 160
        versionName = "0.16.0"
    }

    // Omit AGP's Play-specific DependencyInfoBlock so F-Droid scanner and
    // reproducible-build verification pass without extra signing blocks.
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }

    flavorDimensions += listOf("abi")
    productFlavors {
        create("arm64") {
            dimension = "abi"
            ndk { abiFilters += "arm64-v8a" }
        }
        create("x64") {
            dimension = "abi"
            ndk { abiFilters += "x86_64" }
        }
    }

    signingConfigs {
        val envKsPath = System.getenv("KEYSTORE_PATH")
        val ksFile = if (!envKsPath.isNullOrBlank()) file(envKsPath) else rootProject.file("fieldsmith-release.jks")
        if (ksFile.exists()) {
            create("release") {
                storeFile = ksFile
                storePassword = System.getenv("KEYSTORE_PASSWORD") ?: ""
                keyAlias = System.getenv("KEY_ALIAS") ?: ""
                keyPassword = System.getenv("KEY_PASSWORD") ?: ""
                enableV1Signing = false
                enableV2Signing = true
                enableV3Signing = true
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = signingConfigs.findByName("release")
            vcsInfo.include = false
        }
        debug {
            if (signingConfigs.findByName("release") != null && System.getenv("KEYSTORE_PASSWORD") != null) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }

    lint {
        checkReleaseBuilds = false
        abortOnError = false
    }
}

// Per-ABI versionCode scheme matching Haven / F-Droid VercodeOperation (base * 10 + abiCode):
// arm64 -> 1011, x64 -> 1012.
val abiCodes = mapOf("arm64" to 1, "x64" to 2)
androidComponents {
    onVariants { variant ->
        val abiFlavor = variant.productFlavors.firstOrNull { it.first == "abi" }?.second
        val abiOffset = abiCodes[abiFlavor] ?: 0
        val baseCode = android.defaultConfig.versionCode ?: 101
        variant.outputs.forEach { output ->
            output.versionCode.set(baseCode * 10 + abiOffset)
        }
    }
}

// Sync built `web/dist` assets into `src/main/assets/web` before packaging.
val syncWebAssets = tasks.register<Sync>("syncWebAssets") {
    from(rootProject.file("../web/dist"))
    into(layout.projectDirectory.dir("src/main/assets/web"))
}

tasks.named("preBuild") {
    dependsOn(syncWebAssets)
}
