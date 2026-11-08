# ADR-0011: ライセンス

- **Status:** 採用（Accepted）
- **日付:** 2026-11-08
- **関連:** 実行計画 D-14

## Context

ライセンスは、依存ライブラリとの互換性、プラグインの開発・配布、モバイルストアでの配布に影響する。GPUIはApache-2.0で公開されている（依存先を含めた確認はP0-01で行う）。

## Decision

- Hisui NotesをMIT OR Apache-2.0の二重ライセンスで公開する。リポジトリ直下に `LICENSE-MIT` と `LICENSE-APACHE` を置く。
- 各クレートの `Cargo.toml` に `license = "MIT OR Apache-2.0"` を記載する。
- 依存ライブラリは `cargo-deny` で許可リストを定めて検査する。初期の許可リストはMIT、Apache-2.0、BSD-2-Clause、BSD-3-Clause、ISC、Zlib、Unicode-3.0、MPL-2.0とする。GPL・AGPL・LGPL系は許可しない。
- Zedリポジトリのクレートは、GPUIを含めクレートごとにライセンスを確認してから使用する。

## Alternatives

1. **MPL-2.0。** 本体の改変を公開させられるが、Rustの依存エコシステムでの再利用性が下がる。
2. **GPL-3.0 / AGPL-3.0。** 派生物全体の公開を求められるが、WASMプラグインの扱いの解釈が必要になり、App Storeでの配布に議論がある。

## Consequences

- Rustの標準的な慣行と一致し、依存ライブラリ・GPUIとの互換性の確認が少なくて済む。
- プラグイン作者はライセンスを自由に選べる。
- 第三者が非公開の派生版を作ることを許容する。
- 外部からの貢献を受け入れる際の条件（DCO・CLAの要否）は別途決める。
