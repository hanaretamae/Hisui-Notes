# ADR-0001: GPUIの参照方式とバージョン固定

- **Status:** 採用（Accepted）
- **日付:** 2026-11-08
- **関連:** 統合開発計画 §2.1、§21.1／[`research/P0-01-gpui.md`](../research/P0-01-gpui.md)

## Context

統合開発計画は、全プラットフォームのUIにGPUIを使うと定めている。P0-01の調査で次のことが分かった。

- crates.ioの `gpui` は `0.2.2`（2025-10）のまま更新されていない。現行のGPUIはZedリポジトリ内で複数のクレート（`gpui`、`gpui_platform`、各OSの `gpui_*`）に分かれ、crates.ioに公開されていない。
- iOSバックエンド `gpui_ios` はZedリポジトリに含まれている（実験的）。
- 第三者がZedのタグからcrates.ioへ再配布しているが、公式ではない。
- Zedのワークスペースの `[patch.crates-io]` は、git依存として参照する側に引き継がれない。
- GPUIと、GPUIが使うZed内部のクレートはすべてApache-2.0である。

## Decision

- GPUIは、Zedの公式リポジトリ（`https://github.com/zed-industries/zed`）を、**リリースタグに対応するコミットのハッシュ**で参照する。ブランチ名・タグ名では参照しない（タグの付け替えで内容が変わらないようにするため）。
- 参照するクレートは `gpui` と `gpui_platform`（とその機能フラグ）に限る。Zedの他のクレートを直接使う場合は、ライセンスと必要性を確認してから追加する。
- Zedの `[patch.crates-io]` のうちGPUIの依存に関係するもの（`async-task`、`async-process`、`calloop`、`khronos-egl`）を、Hisui Notesのワークスペースに同じリビジョンで記載する。
- 第三者による再配布クレートは使わない（供給経路の信頼性のため）。
- GPUIの更新は、Zedの安定版リリースに合わせて意図的に行う。更新時は次を確認する。
  1. `[patch.crates-io]` の差分
  2. Zedの `rust-toolchain.toml` が要求するRustのバージョンと、devShellのRustのバージョン
  3. `cargo-deny` によるライセンス・脆弱性の検査
  4. Linuxのビルドと、GPUIに依存する統合テスト（保留中のWindows・macOS・iOSは再開後に対象へ加える）

## Alternatives

1. **crates.ioの `gpui 0.2.2` を使う。** 約1年前の版で、プラットフォーム層の分割、`gpui_ios`、wgpuへの描画の共通化が含まれない。
2. **第三者の再配布クレートを使う。** crates.ioから取得できるが、ビルドの出所を検証できない。
3. **GPUIをフォークしてリポジトリ内に取り込む。** 変更の自由度は高いが、上流への追従の負担が大きい。Androidバックエンドなど上流にない変更が必要になった時点で再検討する。

## Consequences

- 現行のGPUIと `gpui_ios` を使える。
- `Cargo.lock` とgitのコミットハッシュで依存が固定され、再現可能になる。
- GPUIの更新のたびに `[patch]` の照合が必要になる。
- crates.ioに公開しない前提になる（git依存を含むクレートはcrates.ioに公開できない）。ADR-0002の `publish = false` と整合する。
