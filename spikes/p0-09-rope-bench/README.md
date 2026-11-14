# P0-09 ロープ比較ベンチマーク

[`docs/research/P0-09-text-buffer.md`](../../docs/research/P0-09-text-buffer.md) の計測に使ったプログラム。製品コードではない。

```sh
nix develop -c cargo build --release --manifest-path spikes/p0-09-rope-bench/Cargo.toml
spikes/p0-09-rope-bench/target/release/rope-bench ropey
spikes/p0-09-rope-bench/target/release/rope-bench crop
# crop のグラフェムの不具合の最小再現
nix develop -c cargo run --release --manifest-path spikes/p0-09-rope-bench/Cargo.toml --bin diag
```
