# 調査記録: Obsidian Markdownの独自構文

- **対象:** Obsidian Markdown Parser（統合開発計画 §6.1、§6.3）が扱う構文の範囲
- **調査日:** 2026-11-08
- **方法:** Obsidian公式ヘルプの閲覧。実機での確認は、リンク・ブロック識別子（[`obsidian-links.md`](obsidian-links.md) §5）以外は**未実施**。
- **一次資料:**
  - Obsidian Flavored Markdown — https://obsidian.md/help/obsidian-flavored-markdown
  - Basic formatting syntax — https://obsidian.md/help/syntax
  - Advanced formatting syntax — https://obsidian.md/help/advanced-syntax
  - Callouts — https://obsidian.md/help/callouts
  - Tags — https://obsidian.md/help/tags
  - Embed files — https://obsidian.md/help/embeds
  - Properties — https://obsidian.md/help/properties
  - Aliases — https://obsidian.md/help/aliases
  - Internal links — https://obsidian.md/help/links
  - Bases syntax — https://obsidian.md/help/bases/syntax
  - Canvas — https://obsidian.md/help/plugins/canvas
  - Search — https://obsidian.md/help/plugins/search

公式ヘルプは、ObsidianがCommonMark、GitHub Flavored Markdown（GFM）、LaTeXに対応し、そのうち「内部リンク、コメント、ハイライトなど」がObsidian固有であると記載している。各構文がどの由来かを、すべての構文について明記してはいない。

## 1. Obsidian固有の構文

### 1.1 Wikilink

- `[[ノート名]]`、`[[ノート名.md]]`、`[[フォルダ/ノート名]]`（Windowsでも `/`）。Markdown以外のファイルは拡張子が必要（`[[Figure 1.png]]`）。
- 見出し: `[[Note#Heading]]`、サブ見出し: `[[Note#H1#H2]]`、同じノート: `[[#Heading]]`。
- ブロック: `[[Note#^id]]`。
- 表示文字列: `[[Example|Custom name]]`、`[[Example#Details|Section name]]`。
- 表の中では `|` を `\|` でエスケープする（`[[Note\|Alias]]`）。
- `# | ^ : %% [[ ]]` を含む文字列はリンクとして動作しない場合がある。

### 1.2 埋め込み（Embed）

- ノート: `![[Internal links]]`。見出し: `![[Note#Heading]]`。ブロック: `![[Note#^id]]`。
- 画像の大きさ: `![[image.jpg|100x145]]`（幅x高さ）、`![[image.jpg|100]]`（幅のみ、比率維持）。
- 外部画像の大きさ: `![Engelbart|100x145](https://...)`、`![250](https://...)`（代替テキストの位置に大きさを書く。標準のMarkdown画像構文の解釈を変える）。
- PDF: `![[Document.pdf#page=3]]`、`![[Document.pdf#height=400]]`、`![[Document.pdf#page=3&height=400]]`。
- 音声: `![[file.ogg]]`。Canvas: `![[My canvas.canvas]]`。
- `![[image.jpg#outline|100]]` の `#outline` は例にあるが説明がない。

### 1.3 ブロック識別子

[`obsidian-links.md`](obsidian-links.md) を参照。使用可能な文字は英字・数字・ハイフン。引用・コールアウト・表の一部分へのリンクは非対応。

### 1.4 コールアウト

- 引用の1行目に `[!type]` を置く: `> [!info] タイトル`。
- 種類は大文字・小文字を区別しない。未対応の種類は `note` として扱う。
- タイトルは種類の後の文字列。省略すると種類名（先頭大文字）。タイトルだけのコールアウトも可。
- 折り畳み: `> [!faq]- タイトル`（既定で閉じる）、`+`（既定で開く）。
- 入れ子: `> > [!todo] ...` のように `>` を重ねる。
- 種類と別名: note、abstract（summary、tldr）、info、todo、tip（hint、important）、success（check、done）、question（help、faq）、warning（caution、attention）、failure（fail、missing）、danger（error）、bug、example、quote（cite）。
- 独自の種類はCSSで定義する（構文の拡張ではない）。

### 1.5 タグ

- 本文の `#tag`、入れ子 `#inbox/to-read`。プロパティ `tags` のリストでも指定できる。
- 使用可能な文字: 英字、数字、`_`、`-`、`/`（入れ子）、一般的なUnicode文字（絵文字・記号を含む）。日本語のタグを使える。
- 数字以外の文字を1文字以上含む必要がある（`#1984` は無効、`#y1984` は有効）。
- 大文字・小文字を区別しない。空白を含められない。
- `\#` でエスケープする。

### 1.6 プロパティ（YAMLフロントマター）

- ファイル先頭の `---` で囲んだYAML。名前はノート内で一意。
- 型: テキスト、リスト、数値、チェックボックス、日付（`2020-08-21`）、日時（`2020-08-21T10:30:00`）、タグ（`tags` 専用）。
- 型は名前ごとにVault全体で共通。
- テキスト型のリンクは引用符で囲む: `link: "[[Episode IV]]"`。テキストはMarkdownとして描画しない。
- JSON形式も受け付け、保存時にYAMLへ書き換える。
- 既定のプロパティ: `tags`、`aliases`、`cssclasses`（いずれもリスト）。旧名 `tag`、`alias`、`cssclass` は1.9で廃止。
- 入れ子のプロパティ、プロパティ内のMarkdownは非対応（意図的な制限と記載）。

### 1.7 別名（Aliases）

- プロパティ `aliases` に別名を列挙する。リンク候補で別名を選ぶと `[[Artificial Intelligence|AI]]` を生成する。

### 1.8 ハイライト

- `==text==`。開始の `==` の直後に絵文字を置くと色を指定できる: `==🔴Important==`（🔴🟠🟡🟢🔵🟣）。

### 1.9 コメント

- インライン: `%%comment%%`（見出し・リスト項目の中でも可）。
- ブロック: 行頭の `%%` から次の `%%` まで。空行やMarkdownを含められる。
- 閉じていないブロックコメントは、Reading Viewで以降の全文を隠す。
- Editing Viewでのみ表示し、Reading Viewでは表示しない。
- 文字としての `%%` はコードスパンに入れるか、`\%\%` とエスケープする。

### 1.10 インライン脚注

- `^[脚注の本文]`。Reading Viewでのみ動作し、Live Previewでは動作しないと記載されている。

### 1.11 タスクの状態文字

- `- [ ]`、`- [x]` に加え、角括弧内に任意の1文字を書ける（`- [?]`、`- [-]`）。完了扱いになると記載されている。

## 2. 他の仕様に由来する構文（Obsidianが対応しているもの）

| 構文 | 由来 | 記法 |
|---|---|---|
| 表 | GFM | `| a | b |`、区切り行、`:--` `:--:` `--:` |
| 取り消し線 | GFM | `~~text~~` |
| タスクリスト | GFM | `- [ ]`、`- [x]` |
| 脚注 | GFMの拡張（Markdown Extra等） | `[^1]`、`[^1]: 本文`、名前付き `[^note]` |
| 数式 | LaTeX（MathJax） | インライン `$...$`、ブロック `$$...$$`（取得した本文では例の区切り記号が曖昧。要確認） |
| Mermaid | 図表言語 | ` ```mermaid ` のコードブロック |
| YAMLフロントマター | 静的サイト生成器などの慣行 | ファイル先頭の `---` |

## 3. Markdownの構文以外の機能

次はMarkdownの構文ではなく、コードブロックの言語名や別のファイル形式による機能である。

| 機能 | 内容 |
|---|---|
| 検索結果の埋め込み | ` ```query ` のコードブロックに検索式を書く。検索式の文法は検索機能の仕様に依存する |
| Bases | `.base` ファイル（YAML: `filters`、`formulas`、`properties`、`summaries`、`views`）。コードブロックにも埋め込める（言語名は取得した本文では未確認） |
| Canvas | `.canvas` ファイル（JSON Canvas形式、https://jsoncanvas.org/ ）。ノートから `![[x.canvas]]` で埋め込める |
| Mermaidのノードからの内部リンク | Mermaidの `class ... internal-link;` |

## 4. 描画・表示の設定

- **改行:** 公式ヘルプは設定「Strict line breaks」による違いを説明しているが、取得した本文の記述は既定時の動作が曖昧だった。実機で確認する。
- **HTML:** HTML要素の中のMarkdownは描画しない。

## 5. 未確認の事項

- 数式のブロック区切り（`$$`）の正確な規則。
- Basesのコードブロックの言語名。
- 改行の既定動作。
- タスクの状態文字の描画（完了扱い以外の区別があるか）。
- 外部画像の大きさ指定（`![250](url)`）が代替テキストとして他の文字列を伴う場合の規則。
- 埋め込みの `#outline` の意味。
