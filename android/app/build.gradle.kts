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
        versionCode = 170
        versionName = "0.17.0"
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

// Guard against accidentally copying the `fieldsmith-mcp` PIE CLI executable over the
// `libfieldsmith_mcp.so` cdylib (which causes UnsatisfiedLinkError on startLocalServer).
val checkJniSymbols = tasks.register("checkJniSymbols") {
    doLast {
        val soFile = layout.projectDirectory.file("src/main/jniLibs/arm64-v8a/libfieldsmith_mcp.so").asFile
        if (soFile.exists()) {
            val bytes = soFile.readBytes()
            val needle = "Java_sh_fieldsmith_app_FieldsmithNativeBridge_startLocalServer"
                .toByteArray(Charsets.US_ASCII)
            val found = (0..(bytes.size - needle.size)).any { i ->
                var match = true
                for (j in needle.indices) {
                    if (bytes[i + j] != needle[j]) {
                        match = false
                        break
                    }
                }
                match
            }
            if (!found) {
                throw GradleException(
                    "src/main/jniLibs/arm64-v8a/libfieldsmith_mcp.so is missing JNI symbol " +
                        "Java_sh_fieldsmith_app_FieldsmithNativeBridge_startLocalServer " +
                        "(was the CLI binary copied instead of the cdylib? Run `cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release -p fieldsmith-mcp`)."
                )
            }
        }
    }
}

tasks.named("preBuild") {
    dependsOn(syncWebAssets, checkJniSymbols)
}
