plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "dev.shepsky"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.shepsky"
        minSdk = 31
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Signed with the debug key so it installs over a debug build for
            // sideloading. A store build needs a real key.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }
}

// Build the Rust core and drop libshepsky_android.so into jniLibs before the
// Android build packages it. ABIs: -Pshepsky.abis=arm64-v8a,x86_64 to add the
// Intel-emulator one.
val cargoNdkBuild = tasks.register<Exec>("cargoNdkBuild") {
    group = "build"
    description = "Build the Rust core (shepsky-android) for Android"
    workingDir = rootDir.parentFile // repo root
    val abis = (project.findProperty("shepsky.abis") as String? ?: "arm64-v8a")
        .split(',')
        .map { it.trim() }
        .filter { it.isNotEmpty() }
    val args = mutableListOf("ndk")
    abis.forEach { args += listOf("-t", it) }
    args += listOf(
        "-o", "android/app/src/main/jniLibs",
        "build", "-p", "shepsky-android", "--release",
    )
    commandLine("cargo", *args.toTypedArray())
    environment(
        "ANDROID_HOME",
        System.getenv("ANDROID_HOME")
            ?: "${System.getProperty("user.home")}/Library/Android/sdk",
    )
}
tasks.named("preBuild") { dependsOn(cargoNdkBuild) }

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
}
