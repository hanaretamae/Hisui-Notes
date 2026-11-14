// P0-05: GameActivity 版の APK。Rust のライブラリは cargo-ndk で app/src/main/jniLibs に出力する。
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}
rootProject.name = "hisui-p005"
include(":app")
