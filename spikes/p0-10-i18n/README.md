# P0-10 ICU4X・fluent-rsの統合

[`docs/research/P0-10-i18n.md`](../../docs/research/P0-10-i18n.md) の確認に使ったプログラム。製品コードではない。

```sh
cd spikes/p0-10-i18n
nix develop ../.. -c cargo build --release --features all && target/release/i18n-spike
# 機能ごとのサイズ（strip済み）
for f in none segmenter collator normalizer fallback plurals decimal datetime fluent all; do
  a=$([ $f = none ] || echo "--features $f")
  nix develop ../.. -c cargo build --release $a && echo "$f $(stat -c %s target/release/i18n-spike)"
done
```
