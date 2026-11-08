#!/bin/sh
# GameActivity 版の APK を作る。`nix develop .#android` の中で実行する。引数: arm64-v8a | x86_64
set -eu
ABI=${1:-arm64-v8a}
cd "$(dirname "$0")"
cargo ndk -t "$ABI" --platform 35 -o android/app/src/main/jniLibs build --release --no-default-features --features game-activity
cd android
printf 'sdk.dir=%s\n' "$ANDROID_HOME" > local.properties
gradle --no-daemon -q assembleDebug
ls -l app/build/outputs/apk/debug/app-debug.apk
