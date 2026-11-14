# Hisui-Notes

RustとGPUIを基盤とする、マルチプラットフォーム対応のモジュール式文書編集・ノートアプリケーション。

## 計画・設計文書

- [`docs/integrated-plan.md`](docs/integrated-plan.md): 統合開発計画。目的、全体構成、機能範囲、開発工程の上位方針。
- [`docs/development-plan.md`](docs/development-plan.md): 実行計画。要決定事項、Phase 0の作業分解、マイルストーン、ADR候補。
- [`docs/requirements/`](docs/requirements/): 要件。
- [`docs/adr/`](docs/adr/): アーキテクチャ決定記録（ADR）。
- [`docs/research/`](docs/research/): 技術検証・調査記録。
- [`spikes/`](spikes/): 技術検証に使ったプログラム（製品コードではない）。

## 開発環境

Nix flakeで開発環境を定義している（[ADR-0010](docs/adr/0010-nix-development-environment.md)）。

```sh
nix develop
```

Rustツールチェーン（rustc、cargo、clippy、rustfmt、rust-analyzer）と `cargo-deny` が利用可能になる。

## ビルド・実行・検証

devShellの中で実行する（`nix develop -c <コマンド>` でも可）。CI（[`.github/workflows/ci.yml`](.github/workflows/ci.yml)）も同じコマンドを実行する。

```sh
cargo build                                              # ビルド
cargo run -p hisui-notes                                 # アプリの起動（Linux: Wayland・X11）
cargo test --workspace --locked                          # テスト
cargo fmt --all --check                                  # 書式の検査
cargo clippy --workspace --all-targets --locked -- -D warnings  # Lint
cargo xtask deps                                         # クレート間の依存の規則の検査（ADR-0002）
cargo deny check                                         # ライセンス・脆弱性・取得元の検査（ネットワークが必要）
```

## クレート構成

[ADR-0002](docs/adr/0002-crate-structure.md) に従う。仕様は [`docs/spec/`](docs/spec/) に置く。

| クレート | 役割 |
|---|---|
| `app-api` | UIとCoreの間のAPI（Command・Query・Event）。[Spec](docs/spec/app-api.md) |
| `app-core` | `app-api` の実装。GPUIに依存しない |
| `platform` | OS機能の抽象インターフェース（ファイルシステムを含む）とデスクトップの実装 |
| `ui-gpui` | GPUIのフロントエンド |
| `hisui-notes` | 実行ファイル。各層を組み立てる |
| `text-buffer` | Rope、編集、Undo tree、位置変換、書記素境界。[Spec](docs/spec/text-buffer.md) |
| `document` | 文書セッション（ビュー、選択範囲、編集コマンド、IMEの変換中の文字列、保存状態）。[Spec](docs/spec/document-session.md)。文書モデル・`parser-api` はM3 |
| `vault` | Vaultを開く、一覧、読み込み（UTF-8、BOM）、保存（一時ファイルからの置き換え、衝突の検出）。[Spec](docs/spec/vault.md) |
| `parser-markdown` | M3で実装する（現在は空） |
| `xtask` | 開発用の検査 |

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のいずれかを選択できる（[ADR-0011](docs/adr/0011-license.md)）。

## AIエージェント向け文書

- [`AGENTS.md`](AGENTS.md): すべての作業でAIエージェントに読み込ませる共通指示。
- [`docs/AI_DEVELOPMENT.md`](docs/AI_DEVELOPMENT.md): 詳細な開発方針。常時は読み込ませず、MCP、Skills、テスト設計、セキュリティなどの作業で該当する節だけを参照する。
- [`CLAUDE.md`](CLAUDE.md): Claude Code用のアダプター。`AGENTS.md` を読み込むだけの内容。
