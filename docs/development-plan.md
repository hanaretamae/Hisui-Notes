# Hisui Notes — 実行計画

> **位置づけ:** [`integrated-plan.md`](integrated-plan.md)（統合開発計画）を、[`AI_DEVELOPMENT.md`](AI_DEVELOPMENT.md) の文書体系・作業プロセス・検証方針に沿って実行可能な単位へ落とし込む。機能要求や仕様の正本ではない。統合開発計画の内容を変更する提案は「提案」と明記し、承認されるまで統合開発計画を正とする。
>
> **状態:** 草案（2026-11-08）。§2の決定状況は§2.0を参照。

## 1. 文書体系

統合開発計画は要求・アーキテクチャ・工程を一つの文書に含む。着手する領域から順に次の文書へ分割し、分割した節は統合開発計画側を参照リンクに置き換える。

| 文書 | 配置 | 内容 | 作成時期 |
|---|---|---|---|
| 統合開発計画 | `docs/integrated-plan.md` | 目的、全体構成、機能範囲、工程の上位方針 | 作成済み |
| 実行計画 | `docs/development-plan.md` | 本文書。要決定事項、作業分解、マイルストーン | 作成済み |
| Requirements | `docs/requirements/<領域>.md` | 機能・非機能要件、受け入れ条件、未決事項 | 各領域の着手前 |
| Spec | `docs/spec/<領域>.md` | API、データ構造、状態遷移、事前・事後条件、異常系 | 実装前 |
| ADR | `docs/adr/NNNN-<題目>.md` | Context / Decision / Alternatives / Consequences / Status | 決定時 |
| 技術検証記録 | `docs/research/P0-NN-<題目>.md` | Phase 0の検証手順、計測結果、制約、結論 | 各検証の完了時 |

分割の優先順位（Phase 1〜3の着手に必要な順）:

1. `requirements/text-editing.md`、`spec/text-buffer.md`（統合開発計画 §5、§18.4）
2. `requirements/document-model.md`、`spec/document-model.md`（§4）
3. `requirements/vault-and-links.md`、`spec/link-resolution.md`（§8）
4. `requirements/parser.md`、`spec/parser-api.md`（§6）
5. `spec/app-api.md`（§3）

## 2. 要決定事項

統合開発計画の記述間、または記述と工程の間に、着手前に解消が必要な不整合・未定義事項がある。推奨案は筆者の提案であり、§2.0で採用と記載したもの以外は決定ではない。

### 2.0 決定状況（2026-11-08）

| ID | 状態 | 内容 |
|---|---|---|
| D-01〜D-04 | 採用 | 推奨案を採用し、統合開発計画 §6.1、§6.4、Phase 0、2、3、5、6に反映済み |
| パーサーの選定・分離 | 採用 | 標準Markdownパーサーは独自実装。すべてのパーサーを独立したWASMモジュールとし、第三者の拡張とホスト経由で組み合わせる。CJKの句読点に隣接する強調は既定オフの構文機能（[ADR-0004](adr/0004-markdown-parser.md)） |
| WASMランタイム | 採用 | Wasmiを全プラットフォームで使う。JIT・コンパイル結果の保存は行わない。プラグインABIはコアモジュール + 独自ABI（[ADR-0005](adr/0005-wasm-runtime.md)） |
| D-05 | 採用 | 同期は実装しない。編集は範囲置換の操作列として記録し、Undo/Redo・変更通知・クラッシュリカバリーの記録で共有する。操作の位置表現は、将来レプリカ非依存の表現へ変換できる形にする。詳細は [ADR-0003](adr/0003-text-buffer.md) |
| D-06, D-07 | 採用 | 見出しアンカーは見出し文字列そのもの（日本語可）。生成時はURLエンコードせず、必要な場合は山括弧の形式を使う。Obsidian 1.14.5で解決されることを確認済み（[ADR-0009](adr/0009-link-anchor-format.md)、[`research/obsidian-links.md`](research/obsidian-links.md) §5） |
| ブロック識別子 | 採用 | 案B: ASCIIの英字・数字・ハイフンのみ（Obsidian完全互換。ADR-0009） |
| D-08, D-09 | 採用 | 推奨案を採用（ADR-0009 §4）。ファイル移動時のリンク更新は既定で確認してから適用する |
| D-10 | 採用 | プライバシー・セキュリティ優先、性能を考慮した共通権限モデル（[ADR-0008](adr/0008-permission-model.md)） |
| D-11 | 採用 | AI Chatの要件を統合開発計画 §17.4に一本化し、§20.1は参照に置き換えた |
| D-12 | 採用 | 初期クレート構成と分割基準（[ADR-0002](adr/0002-crate-structure.md)） |
| D-13 | 採用（暫定値） | [`requirements/non-functional.md`](requirements/non-functional.md) |
| D-14 | 採用 | MIT OR Apache-2.0（[ADR-0011](adr/0011-license.md)） |
| 開発環境 | 採用 | Nix flakeを開発環境とする（[ADR-0010](adr/0010-nix-development-environment.md)） |
| 対象プラットフォーム | 採用 | Windows・macOS・iOSは指示があるまで保留し、各Phaseの完了条件から除外する（統合開発計画 §1.2） |

### 2.1 工程の依存関係

| ID | 内容 | 該当箇所 | 推奨案 |
|---|---|---|---|
| D-01 | Phase 2でMarkdown標準パーサーとObsidian拡張を実装するが、WASMランタイムはPhase 6。§6.1・§6.4は「WASMとして実行」、Obsidian拡張は「WASMモジュールとして提供」と定める。 | §6.1, §6.4, Phase 2, Phase 6 | `parser-api` をネイティブ実装とWASM実装で共通にする。Phase 2では両パーサーをネイティブで実装し、Phase 6でObsidian拡張をWASMへ移す。標準パーサーもWASMにする（ADR-0004。Phase 2〜5はネイティブで実装し、Phase 6で移す）。 |
| D-02 | Phase 3のLive Preview・Code ViewはGPUI上の描画を前提とするが、GPUI画面基盤はPhase 4。 | Phase 3, Phase 4 | Phase 3に単一ウィンドウの最小エディタービューを含める。モジュール式UI・配置システムはPhase 4のまま。 |
| D-03 | 書記素クラスタ処理とIMEはPhase 5だが、Phase 2のテキストバッファとPhase 3の編集に必須。 | §5.2, Phase 2/3/5 | 書記素境界・位置変換をPhase 2、デスクトップIMEをPhase 3に前倒しする。Phase 5はLocale、Fluent、双方向組版、多言語検索を担当する。 |
| D-04 | Phase 0の完了条件「全対象プラットフォームでGPUI基本画面を起動」は、Androidバックエンドの新規開発を含み、Phase 0の規模を大きくする。 | Phase 0完了条件, §21.2 | Phase 0のAndroid完了条件を「描画・タッチ・テキスト入力のプロトタイプとGo/No-Go判断」に限定する。完成はPhase 9のまま。 |
| D-05 | 同期・CRDTは将来機能だが、採用の有無がテキストバッファ・Undo履歴・文書セッションの設計に影響する。 | §5.2, §22.1 | 同期は実装しないが、編集を操作単位で記録する方式をPhase 2の設計で決める（ADR-0003に含める）。 |

### 2.2 仕様の未定義・不整合

| ID | 内容 | 該当箇所 | 推奨案 |
|---|---|---|---|
| D-06 | 見出しアンカーの生成規則が未定義。GitHub方式のスラッグ化と、Obsidian方式（見出し文字列をそのまま使用）では日本語・空白・記号の扱いが異なる。 | §8.3 | 生成規則をSpecで定義し、両方式を読み取り可能にする。生成形式はADRで決める。 |
| D-07 | `^` はRFC 3986のfragmentで許可されない文字である。`#^block-id` をそのまま出力するか `%5E` で符号化するかが未定義。 | §8.1, §8.4 | 読み取りは両形式を受け付ける。生成形式はObsidian互換性を確認して決める。 |
| D-08 | ブロック識別子はObsidian拡張に属する。Obsidian拡張を無効化した場合のブロックリンクの扱い（保持・未解決表示）が未定義。 | §6.1, §8.4 | 無効化時もリンク文字列を保持し、未解決として扱う（§6.4のフォールバック方針と一致）。 |
| D-09 | ファイル移動時のリンク更新はVault管理と文書形式拡張にまたがる。責務の境界が未定義。 | §8.2, §4.3 | Vaultが移動を通知し、文書形式拡張がリンク書き換えの差分を返す方式をSpecで定義する。 |
| D-10 | MCP（§11.5）、ACP（§12.4）、プラグインCapability（§19.1）、UIモジュール（§14.3）がそれぞれ権限を持つ。共通の権限モデルが未定義。 | §11.5, §12.4, §19.1 | 権限管理を `app-core` の共通基盤とし、各拡張は権限の種類を登録する（ADR-0008）。 |
| D-11 | AI Chatの要件が§17.4と§20.1に重複している。 | §17.4, §20.1 | Requirements分割時に一箇所へ統合する。 |
| D-12 | §2.4のモジュール一覧に責務の重なり（`document`/`document-model`、`locale`/`localization`/`intl`/`unicode`）がある。 | §2.4 | 初期は§5.2のクレート構成に統合し、分割基準をADR-0002に記録する。 |
| D-13 | 性能要件に数値がない（「大きな文書」の規模、入力遅延、起動時間、メモリ）。 | §5.2, §24.2 | Requirementsで目標値を定義する。仮値の例: 10 MBのテキスト文書で入力から描画まで16 ms以内。 |
| D-14 | ライセンスが未決定。依存ライブラリのライセンス方針とプラグイン配布条件に影響する。 | — | Phase 1開始前に決定する。 |

### 2.3 D-13・D-14の決定先

- D-13（性能要件）: [`requirements/non-functional.md`](requirements/non-functional.md)
- D-14（ライセンス）: [ADR-0011](adr/0011-license.md)

## 3. 未確認の前提

次は統合開発計画が前提とするが、本計画の作成時点で検証していない事項である。Phase 0で確認する。

- ~~GPUIの配布形態とバージョン固定方式~~ → 確認済み: Zedリポジトリをコミットハッシュで参照する（ADR-0001）。
- ~~「公式gpui_ios」の存在~~ → 確認済み: Zed本体に実験的な `gpui_ios` がある（2026-10-14マージ）。実機での動作はP0-04で確認する。
- GPUIのテキスト描画基盤が双方向テキスト、複雑な文字体系のシェーピング、フォントフォールバックをどこまで扱えるか。
- GPUIのLinux描画バックエンドの実装方式と、Android（Vulkan / OpenGL ES）への流用可能性。
- Wasmtime + PulleyのAndroidでのビルドと実行可否（iOSは保留）。WasmiのComponent Model対応状況。
- ACPとMCPの現行仕様バージョンと、Rust実装（SDK）の有無。
- Androidのストア規約上（iOSは保留）、WASMプラグインの後からの配布・実行が許容される条件。

## 4. マイルストーン

統合開発計画のPhaseを維持したまま、Linuxデスクトップで縦に通るマイルストーンを置く。開発機がLinuxであるため、Linuxを先行プラットフォームとする。Windows・macOS・iOSは指示があるまで保留する（統合開発計画 §1.2）。

| マイルストーン | 内容 | 対応Phase |
|---|---|---|
| M0 | Phase 0 Tier Aの検証完了とADR-0001〜0005の採択 | Phase 0 |
| M1 | ワークスペース、CI、Core/Platform/Frontendの分離。GPUIの空ウィンドウがCoreのQueryで取得した文字列を表示する | Phase 1 |
| M2 | Vaultを開き、Markdownファイルを読み込み、日本語IMEで編集し、保存できる（Code Viewのみ、Undo/Redo、書記素単位の操作） | Phase 2, 3 |
| M3 | 標準Markdownの解析結果によるReading View、Split View、内部リンク解決、見出し・ブロック参照 | Phase 2, 3 |
| M4 | Live Preview、自動保存・クラッシュリカバリー、Obsidian拡張 | Phase 2, 3 |

M4以降はPhase 4以降の構成に従い、M4完了時に本計画を改訂する。

## 5. Phase 0 作業分解

各検証は `docs/research/` に記録し、結論が長期的な判断を伴う場合はADRを作成する。

### Tier A（Phase 1〜3の設計を左右する）

| ID | 検証内容 | 成果物 | 完了条件 |
|---|---|---|---|
| P0-01 | GPUIのLinuxビルドと起動。バージョン固定方式 | [検証記録](research/P0-01-gpui.md)、ADR-0001 | **完了（2026-11-08）。** 配布形態・ライセンス・Linuxのビルドと起動、niriでのリサイズ時の描画を確認。ウィンドウの最小サイズはアプリ側で設定が必要。Windows・macOSは保留 |
| P0-02 | GPUIのテキスト入力とIME（Linux: Wayland/X11とfcitx5/IBus。Windows・macOSは保留） | [検証記録](research/P0-02-gpui-ime.md) | 日本語の変換中文字列と確定文字列を区別して取得できる。不足点を列挙する。**Waylandで確認済み（2026-11-08、fcitx5）。** 変換中の文字列の中のカーソル位置・文節の情報が渡されない。X11（XWayland）はGPUIのXIMの初期化に競合があり、失敗すると日本語入力ができない（6回中2回）。IBusは未確認 |
| P0-03 | GPUIのテキスト描画（日本語、絵文字、結合文字、アラビア語・ヘブライ語、インド系文字体系、フォールバック） | [検証記録](research/P0-03-gpui-text-rendering.md)、テスト用文字列（`render`） | 文字体系ごとに描画可否と原文位置との対応を記録する。不足機能の補完方針を決める。**完了（2026-11-08、Linux）。** 描画は全文字体系で問題なし。右から左のテキストで位置の変換が成り立たないため、カーソル位置の計算をHisui Notes側で行う方針を提案 |
| P0-06 | WasmiとWasmtime + Pulleyの比較（起動時間、実行速度、メモリ、実行時間・メモリ制限機構、ビルド対象） | [検証記録](research/P0-06-wasm-runtime.md)、ADR-0005 | 同一のMarkdown解析WASMで計測値を比較できる。ADR-0004 §3のモジュールをまたぐ規則の呼び出しを含め、1 MBの再解析がNFR-P03を満たすか計測する。**完了（2026-11-08）。** Wasmiを採用（ADR-0005） |
| P0-08 | Markdownパーサーの選定（既存crateの拡張性・ソース位置・原文保持・インクリメンタル性、または独自実装） | [検証記録](research/P0-08-markdown-parser.md)、ADR-0004 | **完了（2026-11-08）。** 独自実装、パーサーの分離とWASM化を採用（ADR-0004） |
| P0-09 | テキストバッファの選定（Rope実装、書記素境界、UTF-8/UTF-16/コードポイント/書記素位置の変換、操作履歴） | [検証記録](research/P0-09-text-buffer.md)、ADR-0003 | **完了（2026-11-08）。** ropey 1.6.1を採用 |
| P0-14 | Obsidianでの見出しリンク・ブロックリンクの実機確認（[`research/obsidian-links.md`](research/obsidian-links.md) §5） | 調査記録への追記、ADR-0009 | **完了（2026-11-08）。** デスクトップ版で確認。モバイル版は未確認 |

### Tier B（Phase 4〜6の前に必要）

| ID | 検証内容 | 成果物 | 完了条件 |
|---|---|---|---|
| P0-04 | ~~gpui_iosの起動、テキスト入力、編集メニュー~~ | — | **保留**（iOSは指示があるまで保留） |
| P0-05 | Android GPUIバックエンドのプロトタイプ（NDKウィンドウ、Vulkan描画、ALooper、タッチ、IME） | [検証記録](research/P0-05-android-gpui.md)、Go/No-Go判断 | 矩形とテキストの描画、タッチイベント、IME経由の文字入力が動作する。工数見積もりを記録する。**完了（2026-11-09）。Go。** GameActivityで日本語の変換入力まで実機で動作。残りの課題はPhase 9 |
| P0-07 | プラグインのABI方式（ADR-0005によりコアモジュール + 独自ABI）とCapabilityによる権限制御 | [検証記録](research/P0-07-plugin-abi.md)、ADR-0006 | 選定ランタイムで、許可されていないホスト関数の呼び出しを拒否できる。**完了（2026-11-08）。** 読み込み時のインポート検査と呼び出しごとの `check` で25項目すべて期待どおり。ADR-0006を採用 |
| P0-10 | ICU4X・fluent-rsの統合（バイナリサイズ、データの同梱方式、モバイルのビルド） | [検証記録](research/P0-10-i18n.md)、ADR-0007 | 各OS向けビルドのサイズ増加量と、Locale fallbackの動作を記録する。**Linuxで確認（2026-11-08）。** 全機能・全Localeで+5.3 MB（Android arm64で+5.2 MB）。ADR-0007を採用 |
| P0-13 | 同一WASMモジュールのAndroid実行（iOSは保留） | [検証記録](research/P0-13-android-wasm.md) | P0-06のWASMをAndroidで実行し、結果がデスクトップと一致する。**エミュレーター（x86_64）で完了（2026-11-08）。** イベント数・燃料の消費量までデスクトップと一致。実機（arm64）は未確認 |

### Tier C（Phase 7以降の前に必要）

| ID | 検証内容 | 成果物 | 完了条件 |
|---|---|---|---|
| P0-11 | LSP・MCP・ACPの最小クライアント（デスクトップ、stdio。MCPはStreamable HTTPも） | [検証記録](research/P0-11-protocol-clients.md) | 初期化・Capability Negotiation・1往復のリクエストが成立する。**完了（2026-11-09、Linux）。** LSP（marksman）、MCP（stdio・Streamable HTTP）、ACPで成立 |
| P0-12 | Androidのストア規約（iOSは保留）、外部プロセス実行の制約、リモート接続の要件 | [検証記録](research/P0-12-android-store-policy.md) | 規約の該当条項と、プラグイン配布・LSP実行方式への影響を記録する。**完了（2026-11-09）。** WASMプラグインはVM・インタプリタの適用除外に合うと解釈。Androidでは外部プロセスのLSP・stdioのMCPを起動できず、リモート接続が主 |

## 6. Phase 1 着手内容

ADR-0001〜0003採択後に着手する。

### 6.1 開発環境

- Nix flakeのdevShellを開発環境とする（ADR-0010）。Rustツールチェーンはnixpkgsから取得し、`flake.lock` でバージョンを固定する。`flake.nix` は作成済み。
- GPUIのLinuxビルドに必要なシステムライブラリは、P0-01で確定してからdevShellに追加する。
- CIはGitHub Actionsで、Linuxの `cargo fmt --check`、`cargo clippy`、`cargo test`、ライセンス・脆弱性検査（`cargo-deny` 等）を実行する。
- README に環境構築、ビルド、テスト、Lintのコマンドを記載する。

### 6.2 初期クレート構成

[ADR-0002](adr/0002-crate-structure.md) に従う。各クレートの `Cargo.toml` には `license = "MIT OR Apache-2.0"` と `publish = false` を記載する。

### 6.3 完了条件（M1）

- LinuxのCIでビルド・Lint・テストが通る。
- `ui-gpui` がCommandを送り、Eventを受信して表示する統合テストがある。
- `app-api` の型にGPUI・OS固有の型が含まれないことを、依存関係の検査で確認できる。

## 7. ADR候補

| 番号 | 題目 | 決定時期 |
|---|---|---|
| 0001 | GPUIの参照方式とバージョン固定 | 採用済み |
| 0002 | クレート構成と依存規則 | 採用済み |
| 0003 | テキストバッファと編集履歴の方式（同期への備えを含む） | 採用済み |
| 0004 | Markdownパーサーの選定・分離・実行方式 | 採用済み |
| 0005 | WASMランタイムの選定 | 採用済み |
| 0006 | プラグインABIと権限の強制方式 | 採用済み |
| 0007 | 国際化基盤（ICU4X、Fluent）とデータ同梱方式 | 採用済み |
| 0008 | 共通権限モデル | 採用済み |
| 0009 | 内部リンク・見出しアンカー・ブロックIDの形式 | 採用済み |
| 0010 | Nixによる開発環境 | 採用済み |
| 0011 | ライセンス | 採用済み |

## 8. 検証方針

統合開発計画 §24の品質基準と、`AI_DEVELOPMENT.md` §5を次のように適用する。

- 各Specに事前条件・事後条件・不変条件を記載し、テストはSpecから作成する。実装の挙動を期待値として写さない。
- テキストバッファ、位置変換、リンク解決、パーサーにはProperty-based Testingを使う（例: 任意の編集列の後でUndoを全適用すると原文に戻る、位置変換の往復が一致する）。
- パーサーと文書読み込みにはFuzz Testingを導入する（未知の入力で原文を破壊しない、パニックしない）。
- 標準Markdownの適合性はCommonMark spec testsで計測し、通過率を記録する。
- IME・描画・タッチなど自動化できない検証は、手順と結果を `docs/research/` または試験記録に残す。

## 9. 次の作業

1. M0完了（2026-11-08）: Tier Aの検証とADR-0001〜0005の採択がそろった（P0-02のIBus、P0-03の複数行などは未確認の事項として記録済み）。
2. Tier B完了（2026-11-09）: P0-07（ADR-0006）、P0-10（ADR-0007）、P0-13（エミュレーター）、P0-05（Go）。Androidの残りの課題は統合開発計画のPhase 9に記載した。
3. Tier C完了（2026-11-09）: P0-11（LSP・MCP・ACP）、P0-12（ストア規約）。Phase 0の作業分解はすべて完了（iOS関連のP0-04は保留）。
4. Phase 1（M1: ワークスペース、CI、Core/Platform/Frontendの分離）に着手する。
