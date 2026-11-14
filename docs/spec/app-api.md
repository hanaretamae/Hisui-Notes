# Spec: アプリケーションAPI（`app-api`）

- **状態:** 草案（2026-11-09、2026-11-14に版0.2を追加）。M1・M2（実行計画 §4）の範囲。非同期の処理・取り消し・権限・拡張からの利用は後続の版で追加する（§7）。
- **関連:** 統合開発計画 §3.1、§3.2、§3.4／ADR-0002／実行計画 §6.3（M1の完了条件）

## 1. 目的と範囲

UI（Frontend）とアプリケーションコア（Core）の間の共通のAPIを定める。UIはこのAPIだけを通してCoreを操作し、状態の変化をイベントで受け取る。

- Command: 状態を変更する要求。
- Query: 状態を読む要求。副作用を持たない。
- Event: Coreで起きた変化の通知。

## 2. 型の制約

- `app-api` の公開型に、GPUIの型、OS固有の型、文書形式（Markdown等）固有の型を含めない（ADR-0002）。
- `app-api` が依存してよいのは、標準ライブラリと汎用のcrate（`serde`、`futures-channel` 等）のみ。
- すべての公開データ型は `serde` で直列化・復元できる（統合開発計画 §3.4。将来のプロセス間・拡張との受け渡しに備える）。
- 列挙型は `#[non_exhaustive]` とし、副版での追加に備える。

## 3. APIの版

- `API_VERSION`（主版・副版）を公開する。
- 副版の変更では、Command・Query・Event・データ型への追加のみを行う。既存の要素の意味・形式は変えない。
- 主版の変更は互換性のない変更を含む。
- 現在の版: 0.2（主版0の間は互換性を保証しない）。0.2でVault・文書の要素を追加した（§5A）。

## 4. 操作

UIはCoreを `AppApi` として扱う。

| 操作 | 形 | 事前条件 | 事後条件 |
|---|---|---|---|
| `execute(command)` | `Result<(), ApiError>` | なし | `Ok` の場合、変更が状態に反映され、変更に対応するEventが、`execute` が戻る前に全購読者のキューに入っている。`Err` の場合、状態は変更されず、Eventも発行されない |
| `query(query)` | `Result<QueryResponse, ApiError>` | なし | 状態を変更しない。同じ状態に対しては同じ結果を返す |
| `subscribe()` | イベントの受信側（非同期のストリーム） | なし | 呼び出し以降に発行されたEventを、発行の順に受け取る。それ以前のEventは受け取らない |

不変条件:

- Eventは、全購読者に同じ順序で届く。
- 受信側を破棄した購読者には送らない（送信の失敗でCoreの処理を止めない）。

## 5. M1で定義する要素

### 5.1 Query

| Query | 応答 | 内容 |
|---|---|---|
| `AppInfo` | `AppInfo` | アプリ名、アプリの版、APIの版、プラットフォーム名 |
| `StatusMessage` | `StatusMessage` | 現在の状態表示の文字列 |

### 5.2 Command

| Command | 効果 | 異常系 |
|---|---|---|
| `SetStatusMessage { text }` | 状態表示の文字列を `text` にする。`StatusMessageChanged` を発行する。現在と同じ文字列の場合も発行する | `text` が `STATUS_MESSAGE_MAX_BYTES`（4,096バイト）を超える場合は `InvalidArgument`。状態は変更しない |

### 5.3 Event

| Event | 発行の契機 |
|---|---|
| `StatusMessageChanged { text }` | `SetStatusMessage` の成功 |

## 5A. 版0.2で追加する要素（M2: Vault・文書の編集）

関連: [Spec: Vault](vault.md)／[Spec: 文書セッション](document-session.md)

### 5A.1 識別子とデータ型

| 型 | 内容 |
|---|---|
| `DocumentId` | 開いている文書の識別子。Coreが割り当てる |
| `ViewId` | 文書のビューの識別子。M2では文書ごとに1つ |
| `Selection { anchor, head }` | 選択範囲（UTF-8のバイト位置。文書セッション §3） |
| `ViewState { selections, primary, composition }` | ビューの状態。`composition` は変換中の文字列（`range`、`text`、`selected`。文書セッション §6.1）または無し |
| `TextEdit { start, old_len, new_len }` | 内容の変更の1つの範囲置換（バイト位置・長さ）。内容そのものは含めない（受け手はスナップショットで読む。§5A.5） |
| `EditCommand` | 文書セッションの編集コマンド（§5A.2） |

パスはVault相対パス（`/` 区切り。Vault §2）の文字列で受け渡す。

### 5A.2 Command

| Command | 効果 | 異常系 |
|---|---|---|
| `OpenVault { path }` | OSの絶対パスのディレクトリをVaultとして開く。開いている文書はすべて閉じる（未保存の確認はUIが先に行う）。`VaultOpened` を発行する | 開けない場合 `NotFound`・`InvalidArgument` |
| `OpenDocument { path }` | Vault内のファイルを読み込み、文書とビューを作る。`DocumentOpened` を発行する。すでに開いているパスの場合は、その文書とビューで `DocumentOpened` を発行する | Vaultが無い `NotFound`。パスが不正 `InvalidArgument`。UTF-8でない `InvalidData`。シンボリックリンク・ファイルでない `Unsupported` |
| `CloseDocument { document }` | 文書を閉じる（未保存の変更は破棄する。確認はUIが先に行う）。`DocumentClosed` を発行する | `NotFound` |
| `Edit { document, view, command }` | 文書セッションのコマンドを実行する（§5A.2.1）。内容の変更ごとに `DocumentChanged`、ビューの状態の変化で `ViewStateChanged`、未保存の状態の変化で `DirtyChanged` を発行する | 文書・ビューが無い `NotFound`。位置が不正 `InvalidArgument` |
| `SaveDocument { document, overwrite }` | 保存する（Vault §6.2）。`overwrite` が偽なら衝突を検出し、真なら比較せずに上書きする。成功したら `DocumentSaved` と、必要なら `DirtyChanged` を発行する | 衝突 `Conflict`（状態は変えない）。書き込みの失敗 `Io` |
| `ReloadDocument { document }` | ファイルを読み直し、文書セッションの `reload` を行う（Undo可能、保存済みになる）。`DocumentChanged` 等を発行する | `OpenDocument` と同じ |

#### 5A.2.1 EditCommand

文書セッションの操作と1対1に対応する: `InsertText { text }`、`InsertNewline`、`DeleteBackward`、`DeleteForward`、`DeleteSelection`、`MoveCursor { movement, extend }`、`SelectAll`、`SetSelections { selections, primary }`、`Undo`、`Redo`、`SetComposition { text, selected }`、`CommitComposition { text }`、`CancelComposition`。`movement` は `Left`・`Right`・`LineStart`・`LineEnd`・`DocumentStart`・`DocumentEnd`。

### 5A.3 Event

| Event | 発行の契機 |
|---|---|
| `VaultOpened { root }` | `OpenVault` の成功 |
| `DocumentOpened { document, view, path }` | `OpenDocument` の成功 |
| `DocumentClosed { document }` | `CloseDocument` の成功、`OpenVault` による文書の終了 |
| `DocumentChanged { document, version, edits }` | 内容の変更（編集、Undo・Redo、変換の確定、再読み込み）。1つの変更ごとに発行する |
| `ViewStateChanged { document, view, state }` | ビューの選択範囲・変換中の文字列が変わりうる操作の後（他のビューの編集による補正を含む） |
| `DirtyChanged { document, dirty }` | 未保存の状態が変わったとき |
| `DocumentSaved { document }` | `SaveDocument` の成功 |

### 5A.4 Query

| Query | 応答 |
|---|---|
| `VaultFiles` | Vault内のファイル（Vault相対パスの昇順）と、除外した要素の数（Vault §4） |
| `DocumentInfo { document }` | パス、版番号、未保存か |
| `ViewState { document, view }` | ビューの状態 |
| `InputTextForRange { document, view, range }` | 入力用の文字列の範囲（UTF-16位置）の文字列（文書セッション §6.3） |
| `InputSelectedRange { document, view }` | 入力用の主選択範囲（UTF-16位置）と、逆向きか |
| `InputMarkedRange { document, view }` | 変換中の文字列の範囲（UTF-16位置）または無し |
| `InputToBuffer { document, view, offset }`・`BufferToInput { document, view, offset }` | 入力用の位置とバッファのバイト位置の変換 |

### 5A.5 文書の内容の読み出し（同じプロセスのUI向け。2026-11-14 利用者の決定）

文書の内容はCommand・Query・Eventで複製しない（§7「大きなデータ」）。同じプロセスのUIは、`app-core` の公開API（`LocalAppApi::document_snapshot(document)`）で、その時点の不変のスナップショット（text-buffer §7。複製はO(1)）を受け取って読む。

- スナップショットは版番号を持つ。UIは `DocumentChanged` の版番号と比べて、表示が最新かを判断できる。
- `app-api` の型にはスナップショットを含めない（`app-api` は `text-buffer` に依存しない。ADR-0002）。
- プロセスを分ける場合（将来の拡張・リモートのUI）の読み出し方式は、その時点で本Specに追加する。

## 6. エラー

`ApiError` は `code` と、利用者に表示しうる `message` を持つ。

| code | 意味 |
|---|---|
| `InvalidArgument` | 引数が仕様の範囲外 |
| `NotFound` | 対象が存在しない |
| `PermissionDenied` | 権限が無い（ADR-0008） |
| `Unsupported` | このプラットフォーム・版では提供しない |
| `Cancelled` | 取り消された |
| `Internal` | Coreの内部の誤り |
| `Conflict` | 保存時の衝突（版0.2） |
| `InvalidData` | データの形式が不正（UTF-8でないファイルなど。版0.2） |
| `Io` | ファイル等の入出力の失敗（版0.2） |

`message` に文書の内容・シークレットを含めない（ADR-0008 §6）。

診断情報（警告・情報）は `Diagnostic`（重大度、メッセージ、発生元）で表す（統合開発計画 §3.4）。M1ではCommand・Queryの応答には含めない。

## 7. 後続の版で定める事項（未決）

- 長時間の処理・取り消し・進捗（統合開発計画 §3.2）。
- Command・Queryの権限（ADR-0008の主体と権限との対応）。
- 拡張（WASM）からのAPIの利用と、拡張固有のAPIの分離（統合開発計画 §3.3、ADR-0006）。
- 大きなデータ（文書）の受け渡しで複製を避ける方式（統合開発計画 §3.4）。同じプロセスのUIについては §5A.5 で決定した。プロセスを分ける場合は未決。
