# Spec: M2のエディター画面（`ui-gpui`）

- **状態:** 草案（2026-11-14）。M2（実行計画 §4）の範囲。
- **関連:** ADR-0002／[Spec: app-api](app-api.md) §5A／[Spec: 文書セッション](document-session.md)／[Spec: Vault](vault.md)／[NFR](../requirements/non-functional.md) NFR-P02／P0-02・P0-03（`spikes/p0-02-03-gpui-text`）

## 1. 目的と範囲

Vaultを開き、Markdownファイルを選んで、日本語IMEで編集し、保存できる画面（実行計画 §4 M2の完了条件）。Code View（Markdownの原文をそのまま表示する）のみ。

範囲外: 装飾表示（Live Preview）・構文の色分け、複数の文書・タブ・分割、ファイルの作成・名前の変更、検索、自動保存（M4）、設定、テーマ。

## 2. 利用者の決定（2026-11-14）

1. 文書の内容は `app_core::LocalAppApi::document_snapshot` のスナップショットで読む（app-api §5A.5）。
2. 画面は左にファイルの一覧（拡張子 `.md` のみ）、右にエディター。Vaultはコマンドライン引数またはフォルダーの選択ダイアログで開く。同時に開く文書は1つ。
3. 折り返し表示（ソフトラップ）を行う。
4. 未保存の変更がある状態で、他のファイル・Vaultを開く、またはウィンドウを閉じる場合は、「保存」「破棄」「キャンセル」を確認する。保存は Ctrl+S（macOSでは Cmd+S）。
5. 画面の文字はOSのUIフォントで表示する。Linuxでは fontconfig の `sans-serif` の解決結果（`fc-match`）を使い、取得できない場合はGPUIの既定とする。macOS・WindowsではGPUIの既定（`.SystemUIFont`）がOSのフォントに解決される。

草案で提案し、利用者が承認した事項（2026-11-14）:

6. Tab はタブ文字を挿入する（§4.2）。
7. Home・End は論理行の先頭・末尾へ移動する（§4.2）。
8. 保存の衝突時の選択肢は「上書き」「再読み込み」「キャンセル」とする（§5）。
9. Vaultを開く操作に Ctrl+O（macOSでは Cmd+O）を割り当てる。

## 3. 画面の構成

| 領域 | 内容 |
|---|---|
| ツールバー | 「Vaultを開く」ボタン、開いている文書のパスと未保存の印（`●`） |
| ファイルの一覧 | `Query::VaultFiles` の結果のうち `.md` で終わるもの（大文字小文字を区別しない）。クリックで開く。除外した要素がある場合は件数を表示する |
| エディター | §4 |
| 状態表示 | 直前のエラー（Command の失敗）など |

## 4. エディター

### 4.1 表示

- 論理行（改行で区切った行。改行文字は表示しない）を1項目とする仮想化した一覧（GPUIの `list`）で描画し、表示範囲の行だけを組版する（NFR-P02）。
- 各行は表示領域の幅で折り返す（§2-3）。単語の途中の折り返し規則はGPUIの組版に従う。
- 変換中の文字列は、`range` を `text` で置き換えて表示し、下線を引く。`range` が複数行にわたる場合は、最初の行に置き換えた結果を表示し、残りの行を高さ0にする。
- 選択範囲は背景色、主選択のカーソルは縦線で表示する（フォーカスがある場合）。

### 4.2 入力と操作

キー操作は文書セッションのコマンド（app-api §5A.2.1）に対応させる。

| キー | 操作 |
|---|---|
| 文字の入力・IMEの確定 | `CommitComposition`（IMEを介さない入力も同じ。文書セッション §6.2） |
| IMEの変換中の文字列の更新・取り消し | `SetComposition`・`CancelComposition` |
| Backspace・Delete | `DeleteBackward`・`DeleteForward` |
| Enter | `InsertNewline` |
| Tab | タブ文字の挿入 |
| ←・→（Shiftで選択の拡張） | `MoveCursor { Left / Right }` |
| ↑・↓（Shiftで選択の拡張） | 表示上の上下の行の、同じ横位置に最も近い位置へ `SetSelections`（§4.3） |
| Home・End | `MoveCursor { LineStart / LineEnd }`（論理行の先頭・末尾） |
| Ctrl+Home・Ctrl+End | `MoveCursor { DocumentStart / DocumentEnd }` |
| Ctrl+A | `SelectAll` |
| Ctrl+Z・Ctrl+Shift+Z（Ctrl+Y） | `Undo`・`Redo` |
| Ctrl+C・Ctrl+X・Ctrl+V | 主選択範囲の文字列のコピー、コピーと `DeleteSelection`、クリップボードの文字列の挿入 |
| Ctrl+S | 保存（§5） |
| クリック・ドラッグ・Shift+クリック | 位置にカーソルを置く・選択を広げる（`SetSelections`） |

macOSでは Ctrl の代わりに Cmd を使う。

### 4.3 上下の移動

折り返しの結果に依存するため、UIが組版の情報から移動先を決める。移動を続ける間は、最初の横位置（目標の横位置）を保つ。組版の情報が無い行（表示範囲外）へは、論理行の先頭へ移動する。

### 4.4 IME

GPUIの `EntityInputHandler` を、app-api の入力用の Query（§5A.4）と `SetComposition`・`CommitComposition`・`CancelComposition` に対応させる。

- 位置は入力用の文字列のUTF-16位置（文書セッション §6.3）。
- `replace_text_in_range` に範囲が指定された場合は、変換中でなければ、その範囲を選択してから確定する。
- 候補ウィンドウの位置（`bounds_for_range`）は、表示中の行の組版から求める。表示範囲外の場合は返さない。

## 5. 保存と確認

- Ctrl+S: `SaveDocument { overwrite: false }`。
- `Conflict` の場合は「上書き」「再読み込み」「キャンセル」を確認する。上書きは `SaveDocument { overwrite: true }`、再読み込みは `ReloadDocument`（Undoで戻せる）。
- 未保存の確認（§2-4）で「保存」を選び、保存に失敗した場合は、操作を中止して失敗を状態表示に出す。
- 保存は同期的に行う（app-api の既知の制限）。

## 6. エラーの表示

Command・Query の失敗は状態表示に `エラーコード: メッセージ` を表示する。文書を開けない場合（UTF-8でない等）は文書を開かずに表示する。

## 7. 検証

- GPUIのテスト環境（`TestAppContext`）で: Vaultの読み込みと一覧の絞り込み、ファイルを開いて表示の内容がスナップショットと一致すること、キー操作から Command への対応（入力・削除・移動・Undo・保存）、IMEの入力の流れ（変換中の文字列の表示と確定）、未保存の確認と保存の衝突の確認の各選択肢。
- 日本語IME（fcitx5・Wayland）での実際の入力、候補ウィンドウの位置、折り返しの見た目は、利用者の手動確認による（P0-02と同じ手順）。
- NFR-P02の計測は M2 では行わない（計測の仕組みとともに後続で行う）。

## 8. 未決の事項

1. Home・End で表示上の行（折り返し後）の先頭・末尾へ移動する操作を加えるか（M2は論理行。§2-7）。
2. 文字の大きさ・行間（M2は暫定の固定値）。フォントは §2-5 で決定した。
3. 複数の文書・タブ。
