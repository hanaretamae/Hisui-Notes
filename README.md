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

## ライセンス

[MIT](LICENSE-MIT) または [Apache-2.0](LICENSE-APACHE) のいずれかを選択できる（[ADR-0011](docs/adr/0011-license.md)）。

## AIエージェント向け文書

- [`AGENTS.md`](AGENTS.md): すべての作業でAIエージェントに読み込ませる共通指示。
- [`docs/AI_DEVELOPMENT.md`](docs/AI_DEVELOPMENT.md): 詳細な開発方針。常時は読み込ませず、MCP、Skills、テスト設計、セキュリティなどの作業で該当する節だけを参照する。
- [`CLAUDE.md`](CLAUDE.md): Claude Code用のアダプター。`AGENTS.md` を読み込むだけの内容。
