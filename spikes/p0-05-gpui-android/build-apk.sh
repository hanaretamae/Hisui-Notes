#!/bin/sh
# P0-05 の APK を作る。`nix develop .#android` の中で実行する。引数: x86_64 | arm64-v8a
set -eu
ABI=${1:-x86_64}
case $ABI in x86_64) TRIPLE=x86_64-linux-android ;; arm64-v8a) TRIPLE=aarch64-linux-android ;; *) echo "unknown ABI" >&2; exit 1 ;; esac
cd "$(dirname "$0")"
BT="$ANDROID_HOME/build-tools/$(ls "$ANDROID_HOME/build-tools" | sort -V | tail -1)"
cargo ndk -t "$ABI" --platform 35 build --release
rm -rf build/apk && mkdir -p "build/apk/lib/$ABI"
cp "target/$TRIPLE/release/libgpui_android.so" "build/apk/lib/$ABI/"
"$BT/aapt2" link --manifest AndroidManifest.xml -I "$ANDROID_HOME/platforms/android-35/android.jar" -o build/unaligned.apk
# .so は無圧縮で格納し、ページ境界に揃える（extractNativeLibs=false の既定に合わせる）
(cd build/apk && zip -q -0 -r ../unaligned.apk lib)
"$BT/zipalign" -f -p 4 build/unaligned.apk build/aligned.apk
# 検証用のデバッグ鍵（build/ 内に作り、リポジトリには含めない）
[ -f build/debug.keystore ] || keytool -genkeypair -keystore build/debug.keystore -storepass android -keypass android \
  -alias debug -keyalg RSA -keysize 2048 -validity 3650 -dname "CN=Hisui P0-05 Debug" >/dev/null 2>&1
"$BT/apksigner" sign --ks build/debug.keystore --ks-pass pass:android --out build/hisui-p005.apk build/aligned.apk
ls -l build/hisui-p005.apk
