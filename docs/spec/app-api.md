# Spec: アプリケーションAPI（`app-api`）

- **状態:** 草案（2026-11-09）。M1（実行計画 §4）の範囲。非同期の処理・取り消し・権限・拡張からの利用は後続の版で追加する（§7）。
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
- 現在の版: 0.1（主版0の間は互換性を保証しない）。

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

`message` に文書の内容・シークレットを含めない（ADR-0008 §6）。

診断情報（警告・情報）は `Diagnostic`（重大度、メッセージ、発生元）で表す（統合開発計画 §3.4）。M1ではCommand・Queryの応答には含めない。

## 7. 後続の版で定める事項（未決）

- 長時間の処理・取り消し・進捗（統合開発計画 §3.2）。
- Command・Queryの権限（ADR-0008の主体と権限との対応）。
- 拡張（WASM）からのAPIの利用と、拡張固有のAPIの分離（統合開発計画 §3.3、ADR-0006）。
- 大きなデータ（文書）の受け渡しで複製を避ける方式（統合開発計画 §3.4）。
