# ADR-0002: 初期クレート構成と依存規則

- **Status:** 採用（Accepted）
- **日付:** 2026-11-08
- **関連:** 統合開発計画 §2.4、§3.1／実行計画 D-12

## Context

統合開発計画 §2.4は約45のクレートを想定し、同時に「過度なクレート細分化は避ける」と定めている。一覧には責務が重なるもの（`document` と `document-model`、`locale`・`localization`・`intl`・`unicode`）がある。実装開始時点で全クレートを作ると、空のクレートと境界の不確かな公開APIが増える。

## Decision

### 初期クレート

| クレート | 責務 | §2.4の対応 | 依存してよい先 |
|---|---|---|---|
| `app-api` | Command / Query / Event、共通エラー・診断型、APIバージョン | `app-api`、`event-system`の型 | なし（標準ライブラリと汎用crateのみ） |
| `text-buffer` | Rope、編集操作、履歴、位置変換、書記素境界 | `text-buffer`、`unicode`の一部 | なし |
| `document` | 文書モデル、文書セッション、メタデータ、文書形式拡張API、`parser-api` | `document`、`document-model`、`document-format`、`parser-api` | `text-buffer`、`app-api` |
| `vault` | Vault、ファイル入出力、外部変更検出、リンク索引 | `vault`、`storage` | `document`、`platform`、`app-api` |
| `parser-markdown` | 標準Markdown文書形式拡張 | `parser-markdown` | `document` |
| `platform` | OS機能の抽象インターフェースとデスクトップ実装 | `platform`、`platform-linux/windows/macos` | `app-api` |
| `app-core` | コマンド実行、イベント配送、設定、権限（ADR-0008）、拡張管理 | `app-core`、`app-config`、`event-system`の実装 | `ui-gpui`、`hisui-notes` 以外 |
| `ui-gpui` | GPUIフロントエンド | `ui-gpui`、`editor`・`rendering`のUI部分 | `app-api`、`app-core` の公開API |
| `hisui-notes` | 実行ファイル。各層の組み立て | — | すべて |

### 依存規則

- `app-core` 以下のクレートはGPUIに依存しない。
- `document` 以下のクレートはMarkdown固有の型に依存しない。
- `app-api` の公開型にGPUI・OS固有の型を含めない。
- 循環依存を禁止する（Cargoが検出する）。上の依存表に反する依存をCIで検査する。

### 分割の基準

次のいずれかを満たした時点で、§2.4の名前を使ってクレートを分割する。

1. 独立して無効化・交換する単位になった（例: `parser-obsidian`、`lsp`、`mcp`、`acp`）。
2. 依存先が他の部分と異なる（例: ICU4Xを使う `intl`、モバイル固有の `platform-android`）。
3. ビルド時間・テスト範囲を分けたい規模になった。

### 名前

- ワークスペース内のクレート名は§2.4の名前を使う。
- crates.ioへ公開するクレートのみ `hisui-` 接頭辞を付ける。公開は当面行わない（`publish = false`）。

## Alternatives

1. **§2.4の全クレートを最初から作る。** 境界が実装前に固定され、空のクレートが増える。
2. **単一クレートで始める。** GPUI非依存の境界をコンパイラーで強制できない。

## Consequences

- Core・Platform・Frontendの境界と、文書形式非依存の境界をクレート依存で強制できる。
- `document` と `app-core` は責務が大きくなる。分割の基準に達した時点で分割し、本ADRの表を更新する。
