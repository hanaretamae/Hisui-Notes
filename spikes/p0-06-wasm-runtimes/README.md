# P0-06 WASMランタイム比較

[`docs/research/P0-06-wasm-runtime.md`](../../docs/research/P0-06-wasm-runtime.md) の計測に使ったプログラム。製品コードではない。

- `guest/`: 計測用のWASMモジュール（`wasm32-unknown-unknown`、WASIなし）。Markdownの解析の負荷としてpulldown-cmarkを使う。
- `host/`: Wasmi、Wasmtime（Cranelift・Pulley）でゲストを実行して計測する。

devShellには `wasm32-unknown-unknown` 用のリンカー（`lld`）が無いため、計測時は `nix shell nixpkgs#lld` で一時的に追加した。

```sh
cd spikes/p0-06-wasm-runtimes
# 1 MBの文書（P0-08と同じ生成器）
(cd ../p0-08-markdown-parsers && nix develop ../.. -c cargo build --release && target/release/md-parsers doc) > doc-1mb.md
# ゲスト
(cd guest && nix develop ../../.. -c nix shell nixpkgs#lld -c cargo build --release --target wasm32-unknown-unknown)
# ホスト
cd host && nix develop ../../.. -c cargo build --release
B=target/release/wasm-host
$B precompile                 # ../guest.pulley64.cwasm を生成
taskset -c 2 $B startup       # コンパイル・インスタンス化の時間とメモリ
taskset -c 2 $B bench         # 1 MB・4 KBの解析、ホスト経由の呼び出し
taskset -c 2 $B limits        # 燃料・エポック・メモリ上限
# バイナリサイズ（ランタイムごとの構成）
for f in none wasmi wasmtime-cranelift wasmtime-pulley-runtime; do
  feat=$([ $f = none ] && echo --no-default-features || echo "--no-default-features --features $f")
  CARGO_TARGET_DIR=target/size-$f nix develop ../../.. -c cargo build --release $feat \
    --config profile.release.debug=0 --config profile.release.strip=true
done
```

計測機はPコアとEコアを持つため、`taskset` で1つのPコアに固定して計測した。

## Android（P0-13）

```sh
# エミュレーター（初回のみ AVD を作成）
nix develop .#android -c sh -c 'echo no | avdmanager create avd -n hisui-api35 -k "system-images;android-35;default;x86_64" -d pixel_7'
nix develop .#android -c emulator -avd hisui-api35 -no-window -no-audio -no-snapshot -no-boot-anim -gpu swiftshader_indirect &
# ビルドと実行（spikes/p0-06-wasm-runtimes/host で）
nix develop ../../..#android -c cargo ndk -t x86_64 --platform 35 build --release --no-default-features --features wasmi
D=/data/local/tmp/hisui
adb shell mkdir -p $D/host
adb push target/x86_64-linux-android/release/wasm-host $D/host/
adb push ../guest/target/wasm32-unknown-unknown/release/wasm_guest.wasm ../doc-1mb.md $D/
adb shell "cd $D/host && ./wasm-host bench ../wasm_guest.wasm ../doc-1mb.md"
```
