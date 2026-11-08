# P0-07 プラグインABIと権限制御

[`docs/research/P0-07-plugin-abi.md`](../../docs/research/P0-07-plugin-abi.md) の検証に使ったプログラム。製品コードではない。

- `guest/`: ABI v1 に従うRustのプラグイン（`wasm32-unknown-unknown`、WASIなし）。
- `host/`: Wasmi 2.0.0 のホスト。Permission Broker（ADR-0008）の最小実装、読み込み時のインポートの検査、ホスト関数、検証の場面（不正なモジュールはWATで記述）。

```sh
cd spikes/p0-07-plugin-abi
(cd guest && nix develop ../../.. -c nix shell nixpkgs#lld -c cargo build --release --target wasm32-unknown-unknown)
cd host && nix develop ../../.. -c cargo build --release
taskset -c 2 target/release/plugin-host   # すべて PASS なら終了コード 0
```
