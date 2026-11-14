# P0-02・P0-03 GPUIのテキスト入力・描画の確認

[`docs/research/P0-02-gpui-ime.md`](../../docs/research/P0-02-gpui-ime.md)、[`docs/research/P0-03-gpui-text-rendering.md`](../../docs/research/P0-03-gpui-text-rendering.md) の確認に使ったプログラム。製品コードではない。

GPUIはADR-0001の方式（Zedのコミットの参照と `[patch.crates-io]`）で参照する。

- `ime`: Zedの `crates/gpui/examples/input.rs`（Apache-2.0）を複製し、入力ハンドラーの呼び出しを標準エラー出力に記録するようにしたもの。
- `render`: 各文字体系の文字列を表示し、整形の結果（字形なし、表示の順序、位置の変換の往復）を標準出力に書く。

```sh
nix develop -c cargo build --manifest-path spikes/p0-02-03-gpui-text/Cargo.toml
# Wayland
nix develop -c spikes/p0-02-03-gpui-text/target/debug/ime 2> ime-wayland.log
# X11（XWayland）
WAYLAND_DISPLAY= nix develop -c spikes/p0-02-03-gpui-text/target/debug/ime 2> ime-x11.log
# 描画（--analyze-only で解析だけ行って終了）
nix develop -c spikes/p0-02-03-gpui-text/target/debug/render
```
