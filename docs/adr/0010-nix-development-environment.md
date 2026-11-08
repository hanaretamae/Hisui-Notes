# ADR-0010: Nixによる開発環境

- **Status:** 採用（Accepted）
- **日付:** 2026-11-08
- **関連:** `AI_DEVELOPMENT.md` §6.1／実行計画 §6.1

## Context

`AI_DEVELOPMENT.md` §6.1は、言語ランタイム、依存パッケージ、システムツールを宣言的に管理し、人間とAIエージェントが同じ環境を再現できるようにすることを求めている。開発機はNixOSである。GPUIのLinuxビルドには、Rustツールチェーンのほかにシステムライブラリ（Wayland、X11、xkbcommon、Vulkan、fontconfigなど。正確な一覧はP0-01で確定する）が必要になる。

## Decision

- 開発環境をNix flakeのdevShellで定義する（`flake.nix`、`flake.lock`）。
- Rustツールチェーン（rustc、cargo、clippy、rustfmt、rust-analyzer）はnixpkgsから取得し、バージョンは `flake.lock` で固定する。
- GPUIなどが要求するシステムライブラリは、必要が確認された時点でdevShellに追加する。
- 開発環境には `nix develop` で入る。

## Alternatives

1. **rustup + `rust-toolchain.toml`。** Rust以外のシステムライブラリを宣言的に管理できない。
2. **Nix + rust-overlayまたはfenix。** nixpkgsにない特定のRustバージョン（nightly、特定のstable）を使える。外部flake入力が増える。GPUIがnixpkgsのRustより新しいバージョンを要求した場合に再検討する。
3. **Dev Container。** NixOS上での利用に追加の仕組みが要る。

## Consequences

- NixOSの開発機と、Nixを導入した他のLinux・macOSで同じ環境を再現できる。
- Windows開発者はNixを使えない。Windowsは保留中（統合開発計画 §1.2）のため、再開時にWindowsのビルド手順を決める。
- CIでもNixを使うか、各OSの標準ツールチェーンを使うかはPhase 1のCI構築時に決める。
- Rustのバージョン更新は `nix flake update` で行い、差分をレビューする。

## 改訂（2026-11-08）: Android向けのdevShell

- **Context:** P0-05・P0-13でAndroid向けのビルドと実行が必要になった。nixpkgsの `rustc` はホストと `wasm32` の標準ライブラリしか含まず、Android向けの標準ライブラリを得るにはコンパイラーの再ビルドが必要になる。
- **Decision:**
  - x86_64 Linux向けに別のdevShell `android`（`nix develop .#android`）を追加する。`default` シェルは変更しない。
  - `android` シェルのRustツールチェーンは、flake入力 `rust-overlay` の公式配布版を使い、版は `default` シェルのnixpkgsの `rustc` と同じ（1.99.0）に固定する。対象は `x86_64-linux-android`（エミュレーター）、`aarch64-linux-android`（実機）、`wasm32-unknown-unknown`。
  - Android SDK・NDK（28.2.13676358）・エミュレーター・システムイメージ（API 35、x86_64）はnixpkgsの `androidenv` から取得する。`cargo-ndk`、`lld`、APKの作成用の `jdk17_headless`・`zip`・`gradle_8` を含める。
  - Android SDKの利用条件（Android SDK License）への同意を、`android` シェルの設定（`android_sdk.accept_license = true`）として記録する。利用者の指示（2026-11-08）による。
- **Consequences:** flake入力が1つ増える。2つのシェルでRustの版を手動で揃える必要がある（`nix flake update` の後に確認する）。`android` シェルの初回の取得は数GBになる。
