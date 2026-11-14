# ADR-0005: WASMランタイムの選定

- **Status:** 採用（Accepted）
- **日付:** 2026-11-08
- **関連:** 統合開発計画 §6.4、§19／実行計画 P0-06、P0-07／[`research/P0-06-wasm-runtime.md`](../research/P0-06-wasm-runtime.md)／ADR-0004、ADR-0008、ADR-0011／[`requirements/non-functional.md`](../requirements/non-functional.md) NFR-P03、NFR-P04、NFR-P08

## Context

ADR-0004で、すべてのパーサーをWASMモジュールとして実行し、第三者の拡張とホスト経由で組み合わせることを決めた。ユーザープラグインもWASMを基本とする（統合開発計画 §19）。iOSではJITを使えない。プライバシー・セキュリティを性能より優先する（requirements/non-functional.md）。

P0-06で、Wasmi 2.0.0、Wasmtime 50.0.1 + Pulley、Wasmtime + Cranelift（参考）を、pulldown-cmarkをWASMにしたモジュールで比較した（Linuxデスクトップ、Pコアに固定）。

- 実行速度（ネイティブ比）: Cranelift 約1.6倍、Wasmi 約14倍（4 KBのブロック解析は約0.9 ms）、Pulley 約39倍。
- ホスト経由で別のインスタンスの関数を呼ぶ費用: Wasmi 約0.3 µs。
- モジュールの準備: Wasmi 2〜9 ms（コンパイル結果の保存は不要）、Wasmtime 約0.36秒（保存したコンパイル結果の読み込みは `unsafe` で、内容を信頼できることが前提）。
- 上限: 3構成とも燃料とメモリ上限で停止できた。燃料による負荷はWasmi約+3%、Wasmtime約+23%。壁時計による中断（エポック）はWasmtimeのみ。
- バイナリの増加: Wasmi +2.5 MB、Wasmtime（コンパイラー込み）+11.6 MB。
- Component Model: Wasmtimeは対応。Wasmi 2.0.0には記載が無い。

## Decision

- WASMランタイムに **Wasmi** を使い、全プラットフォームで同じ実装を使う。初期の版は計測した2.0.0とし、更新は `Cargo.lock` で管理する。
- JITとコンパイル結果のディスクへの保存は行わない。
- 実行時間の上限（NFR-P08）は燃料（命令数）で設ける。壁時計の時間に対する上限が必要な場合は、計測した換算に基づいて燃料の値を決める。燃料の値はランタイムの版の更新時に較正し直す。
- メモリの上限は `StoreLimits` で設ける。
- 上限による停止の後もホストは処理を続け、ADR-0004 §3のフォールバック（その拡張が無いものとして解析を続ける）を行う。
- ホスト側では、ランタイムに依存する部分を1つの層（`app-core` 内）に閉じ込め、Wasmiの型を他の層の公開APIに出さない。将来デスクトップで性能が不足した場合に、別のランタイムを追加できるようにする。
- WASMモジュールのビルド対象は `wasm32-unknown-unknown` とし、WASIを使わない（ADR-0008）。devShellに `wasm32-unknown-unknown` 用のリンカー（`lld`）を追加する（ADR-0010の更新。パーサーをWASMでビルドする作業の開始時に行う）。

## Alternatives

1. **Wasmtimeを使い、デスクトップ・AndroidはCranelift、iOSはPulleyにする。** 最も速く、Component Modelを使える。JITの攻撃面、コンパイル結果の保存とその検証、バイナリサイズ、プラットフォームによる実行方式の違いを負う。
2. **Wasmtime + Pulleyだけを使う。** バイナリは最小だが、計測で最も遅く、NFR-P03の余裕が小さい。第三者の拡張にも事前コンパイルが必要になる。

## Consequences

- 全プラットフォームで同じ挙動になり、iOSでも同じ方式で動く。攻撃面が小さい。
- Component Model（WIT）を使えない。**P0-07のプラグインABIは、コアモジュール + 独自ABIとする**（方式の詳細と権限制御はP0-07で検証し、ADR-0006で決める）。
- 1 MBの文書を開いたときの全体解析に約0.2秒（計測機）かかる。表示中の範囲を優先して解析する設計（requirements/parser.md PR-12、PR-38）と、ブロック単位の再解析（ADR-0004）が前提になる。
- 壁時計による中断が無いため、燃料の値の較正が必要になる。
- モバイル端末での性能はP0-13で確認する。
