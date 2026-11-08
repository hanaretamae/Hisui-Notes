plugins {
    id("com.android.application")
}

android {
    namespace = "org.hisuinotes.p005"
    compileSdk = 35
    // nix の SDK は読み取り専用のため、SDK にある版を指定する（ダウンロードさせない）
    buildToolsVersion = "37.0.0"
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "org.hisuinotes.p005"
        minSdk = 30
        targetSdk = 35
        versionCode = 1
        versionName = "0.0"
    }
    // build-apk.sh と同じ検証用のデバッグ鍵で署名する（端末上で同じアプリとして上書きできるように）
    signingConfigs {
        getByName("debug") {
            storeFile = file("../../build/debug.keystore")
            storePassword = "android"
            keyAlias = "debug"
            keyPassword = "android"
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    // AndroidX の依存が古い kotlin-stdlib-jdk7/jdk8 を持ち込み、クラスが重複するため版を揃える
    implementation(platform("org.jetbrains.kotlin:kotlin-bom:1.8.22"))
    // android-activity 0.6 が対応する版（同crateのREADME）
    implementation("androidx.games:games-activity:4.4.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.core:core:1.16.0")
}
