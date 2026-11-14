# P0-08 Markdownパーサー比較

[`docs/research/P0-08-markdown-parser.md`](../../docs/research/P0-08-markdown-parser.md) の計測に使ったプログラム。製品コードではない。

CommonMark spec tests（0.31.2）はリポジトリに含めない。実行前に `data/` へ取得する（`data/` はGitの管理外）。

```sh
cd spikes/p0-08-markdown-parsers
mkdir -p data && curl -fo data/spec.json https://spec.commonmark.org/0.31.2/spec.json
# 計測時のSHA-256: d431b29d97b6f73e69d547109cf5081578fac931e72afe95639ebe766c1b2a20
nix develop ../.. -c cargo build --release

B=target/release/md-parsers
$B spec data/spec.json           # 適合率。SPEC_DIFF=pd|cm|mr|mi で不一致の内容を表示
$B bench                         # 1 MBの文書の解析時間
$B positions                     # リンク・見出しの原文範囲
$B obsidian                      # Obsidian構文の組み込み対応
$B ext                           # 構文の追加（markdown-itの規則、pulldown-cmarkの後処理）
$B patho                         # 病的な入力の一覧
for i in $(seq 0 11); do for p in pd cm mr mi; do timeout 15 $B patho $i $p || echo "$i $p timeout"; done; done
```
