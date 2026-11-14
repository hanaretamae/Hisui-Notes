# Spec: 標準Markdownパーサー（`parser-markdown`）

- **状態:** 草案（2026-11-14）。M3（実行計画 §4）の最初の作業。§5の制限と§7の判断は承認待ち。
- **関連:** ADR-0002／ADR-0004／[requirements/parser.md](../requirements/parser.md)／[NFR](../requirements/non-functional.md) NFR-P03／[P0-08 検証記録](../research/P0-08-markdown-parser.md)

## 1. 目的と範囲

CommonMark spec 0.31.2 に基づく標準Markdownパーサーを独自に実装する（ADR-0004 §1）。M3のReading ViewとSplit Viewは、このパーサーの構文木を使う。

本Specの範囲（最初のPR）:

- CommonMark spec 0.31.2 のすべての構文の解析。
- 原文のバイト位置を持つ構文木（§3）。
- 検証用のHTMLへの変換（§4.2）。
- 解析にかかる時間とスタックの深さの上限（§5）。

範囲外（後続の作業。§9）: 拡張フックと `parser-api`（ADR-0004 §3）、編集箇所の再解析（NFR-P03）、GFMなどの拡張構文、`cjk-friendly-emphasis`（ADR-0004 §5）、Obsidian Markdown（`parser-obsidian`）、WASMでの実行（Phase 6）。

## 2. 利用者の決定（2026-11-14）

1. CommonMark spec tests のデータ（CC BY-SA 4.0）をリポジトリに含める（ADR-0004 §6の「取得方法とライセンスの扱い」）。データは `crates/parser-markdown/tests/data/` に置き、著作者・取得元・ライセンスを同じ場所の README に記す。
2. Reading View は CommonMark と最小限の Obsidian 構文（wikilink、見出しの参照、ブロックID）を表示する。Obsidian 構文は `parser-obsidian` で実装し、本Specには含めない。

## 3. 構文木

`parse(text: &str) -> Node` は文書全体の構文木を返す。

```rust
pub struct Node {
    pub kind: NodeKind,
    pub range: Range<usize>, // 原文の UTF-8 のバイト位置
    pub children: Vec<Node>,
}
```

### 3.1 節の種類（`NodeKind`、`#[non_exhaustive]`）

| 種類 | 内容 |
|---|---|
| `Document` | 文書。`range` は原文全体 |
| `BlockQuote` | ブロック引用 |
| `List(ListInfo)` | リスト。`ordered`・`start`（番号付きの開始番号）・`marker`（`-` `+` `*` `.` `)`）・`tight` |
| `Item` | リストの項目 |
| `Paragraph` | 段落 |
| `Heading { level, setext }` | 見出し（レベル1〜6）。`setext` は下線による見出し |
| `ThematicBreak` | 区切り線 |
| `CodeBlock { info, literal }` | コードブロック。`info` はフェンスの情報文字列（インデントによる場合は `None`）、`literal` は内容 |
| `HtmlBlock { literal }` | HTMLブロック |
| `Text(String)` | 文字列（バックスラッシュエスケープ・文字参照を解決済み） |
| `SoftBreak`・`HardBreak` | 改行 |
| `Code(String)` | コードスパン |
| `Emphasis`・`Strong` | 強調 |
| `Link { destination, title }`・`Image { destination, title }` | リンク・画像。`destination` はエスケープ・文字参照を解決済みで、URLの正規化（パーセントエンコード）はしない |
| `HtmlInline(String)` | インラインのHTML |

リンク参照定義は構文木に含めない（定義だけの段落は取り除く）。

### 3.2 位置

- 位置は UTF-8 のバイト位置で、常に文字の境界にある（ADR-0004 §1）。
- ブロックの範囲は、ブロックの開始位置（ブロック引用の `>`、リストの記号、段落の最初の文字など）から、最後の行の行末（改行を含まない）まで。
- インラインの範囲は、記号を含む原文の範囲（`*強調*`、`[リンク](u)` 全体）。
- 子の範囲は親の範囲に含まれ、兄弟の範囲は重ならず、原文の順に並ぶ。
- `Text` の範囲は対応する原文の範囲で、エスケープ・文字参照・改行を含まない場合は内容と一致する。

### 3.3 原文の扱い

- 原文は変更しない。改行（LF・CRLF・CR）の違いは解析の結果に影響しない。
- U+0000 は構文木の文字列でだけ U+FFFD に置き換える（spec 2.3）。位置は変えない。
- どの入力でも失敗しない（CommonMark ではすべての文字列が正しい文書）。

## 4. API

### 4.1 解析

`parser_markdown::parse(text) -> Node`。状態を持たず、OSの機能（ファイル・スレッド・時刻）を使わない（ADR-0004 §4）。

### 4.2 HTMLへの変換

`parser_markdown::html::render(&Node) -> String` は、CommonMark の参照実装（commonmark.js）と同じ形式のHTMLを返す。spec tests と差分試験に使う。Reading View の描画には使わない（Reading View は構文木を直接描画する）。

## 5. 制限（草案。承認待ち）

病的な入力で時間とスタックが際限なく増えないように、次の上限を設ける。上限を超えた部分は、その構文として解析しない（原文は失われず、文字列として残る）。

| 上限 | 値 | 超えた場合 | spec との関係 |
|---|---|---|---|
| ブロックの入れ子の深さ（ブロック引用・リスト） | 100 | それより深い `>`・リストの記号は段落の文字列になる | spec に無い制限 |
| インラインの入れ子の深さ（強調・リンク） | 100 | それより深い強調・リンクを作らない | spec に無い制限 |
| リンク先の括弧の入れ子 | 32 | リンクにならない | spec 6.3 が実装に認める制限 |
| リンクラベルの長さ | 999文字 | 参照として扱わない | spec 4.7 の規則 |

計算量: コードスパンの閉じの検索、閉じていないHTMLコメント・処理命令・CDATA・宣言の検索は、同じ範囲を繰り返し走査しない（入力の長さに対しておおむね線形）。

計測（参考、リリースビルド、2026-11-14）: 約1.3 MBの通常の文書の全体解析は約15 ms（pulldown-cmark は約4 ms）。spec の例を連結した約1 MBの文書は約130 ms（同 約45 ms）。NFR-P03（1文字の編集後の再解析 5 ms）は編集箇所の再解析（§9）で満たす。

## 6. 性質

試験（Property-based Testing）で次を確かめる。

1. 位置は原文の範囲内で、文字の境界にある。子は親に含まれ、兄弟は重ならず原文の順に並ぶ（§3.2）。
2. エスケープ等を含まない `Text` の内容は原文と一致する。
3. 改行を LF・CRLF・CR のどれにしても、HTMLは同じ。
4. 任意の Unicode の入力で panic しない。
5. ブロック・インラインの入れ子の深さが §5の上限を超えない。

## 7. 参照実装と結果が分かれる入力の扱い（草案。承認待ち）

差分試験（§8）で、参照実装どうしでも結果が分かれる入力が見つかった。spec の記述に沿うものを採り、spec が決めていないものは cmark（C の参照実装）または commonmark.js に合わせる。

| 入力の例 | 本実装 | 根拠・同じ結果の実装 |
|---|---|---|
| 遅延継続の行の先頭の空白（`` > ``a\n  b`` ``） | 残す（`a   b`） | cmark・pulldown-cmark・markdown-rs |
| リストの項目の中のコードブロックの、字下げの深い空行 | 項目の字下げを除いた残りの空白を残す | cmark（項目の継続の判定で字下げを空行より先に見る） |
| 遅延継続の行の type 7 のHTML（`- a\n<span>`） | 段落の継続とする | spec 4.6（type 7 は段落を中断しない）、commonmark.js・pulldown-cmark。cmark・markdown-rs はHTMLブロックとする |
| リンク参照定義の次の行の `---`（`[r]: /u\n---`） | 区切り線 | 定義は段落ではない（spec 4.7）。commonmark.js・pulldown-cmark。cmark・markdown-rs は段落とする |
| 空白だけの括弧（`[r][   ](u)`） | `[r]` は省略形の参照、`[   ](u)` はリンク | spec 6.3（ラベルは空白以外の文字を含む）、pulldown-cmark |
| 定義だけの段落と、空行の後の別のブロックを含む項目 | loose | spec 5.3（定義もブロック）、pulldown-cmark・markdown-rs |
| 改行の無い最後の行が空白だけ | 空行 | spec 4.9 |
| 改行の無い最後の行で終わるコードブロック | 内容の末尾に改行を加えない | cmark |
| 画像の代替テキストの中のHTML | エスケープして残す | cmark |

## 8. 検証

| 試験 | 内容 |
|---|---|
| `tests/commonmark_spec.rs` | spec 0.31.2 の全652例のHTMLが一致する |
| `tests/differential.rs` | comrak・pulldown-cmark・markdown-rs（`dev-dependencies` のみ）の少なくとも1つとHTMLが一致する（ADR-0004 §6）。HTMLの書式の差（引用符の書き方、末尾の改行、コードスパンの連続する空白など）は比較の前に揃える。§7の入力があるため、CIでは乱数の種を固定する |
| `tests/pathological.rs` | P0-08 §3.6・cmark の pathological tests を参考にした21種の入力が時間の上限内に終わる |
| `tests/properties.rs` | §6の性質 |
| `tests/regressions.rs` | 差分試験で見つかった差の回帰試験 |

差分試験は調査時に `PROPTEST_CASES` を増やして実行する（2026-11-14 に種を変えて計数百万例を実行し、残る差は§7のものだけだった）。

spec tests のデータの扱いは §2-1。データは試験にだけ使い、製品のバイナリには含めない。

## 9. 未決の事項・後続の作業

1. 拡張フックと `parser-api`（ADR-0004 §3。`parser-api` は `document` クレートに置く。ADR-0002）。
2. 編集箇所を含むブロックだけの再解析と、文書全体に影響する構文（リンク参照定義、閉じていないコードブロック・HTMLブロック）の扱い（ADR-0004 §1、NFR-P03）。
3. `cjk-friendly-emphasis`（ADR-0004 §5）。
4. Fuzz Testing（ADR-0004 §6。現在は Property-based Testing の任意の Unicode の入力のみ）。
