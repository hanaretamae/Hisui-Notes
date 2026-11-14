# P0-05 Android向けGPUIバックエンドのプロトタイプ

[`docs/research/P0-05-android-gpui.md`](../../docs/research/P0-05-android-gpui.md) の確認に使ったプログラム。製品コードではない。

- `src/platform.rs`・`src/window.rs`・`src/dispatcher.rs`: GPUIの `Platform`・`PlatformWindow`・`PlatformDispatcher` のAndroid向け実装（NativeActivity、`android-activity`）。
- `src/demo.rs`: デモ。Zedの `crates/gpui/examples/input.rs`（Apache-2.0）を基にしたP0-02の `ime` を複製し、Android向けに変更したもの。
- `build-apk.sh`: Gradleを使わずにAPKを作る（`aapt2`、`zipalign`、`apksigner`）。検証用のデバッグ鍵は `build/` に作る（Gitの管理外）。

```sh
nix develop .#android -c spikes/p0-05-gpui-android/build-apk.sh x86_64
nix develop .#android -c emulator -avd hisui-api35 -no-window -no-audio -no-snapshot -no-boot-anim -gpu swiftshader_indirect &
nix develop .#android -c sh -c 'adb install -r spikes/p0-05-gpui-android/build/hisui-p005.apk && adb shell am start -n org.hisuinotes.p005/android.app.NativeActivity'
nix develop .#android -c adb logcat -s hisui:*
```

## GameActivity版（IMEの変換入力。実機で確認）

`android/` はGradleのプロジェクト（AndroidXの `games-activity` 4.4.0）。Rustのライブラリは `cargo ndk -o` で `android/app/src/main/jniLibs` に出力する。

```sh
nix develop .#android -c spikes/p0-05-gpui-android/build-gradle-apk.sh arm64-v8a
nix develop .#android -c sh -c 'adb install -r spikes/p0-05-gpui-android/android/app/build/outputs/apk/debug/app-debug.apk && adb shell am start -n org.hisuinotes.p005/.MainActivity'
```
