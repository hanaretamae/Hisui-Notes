# P0-11: LSP・MCP・ACPの最小クライアント

- **調査日:** 2026-11-09
- **状態:** 完了（デスクトップ、Linux）。
- **目的:** 統合開発計画 §9（LSP）、§11（MCP）、§12（ACP）の接続の基盤として、各プロトコルの最小クライアントで「初期化・Capabilityの交換・1往復の要求」が成立することを確認する（実行計画 P0-11の完了条件）。
- **確認プログラム:** [`spikes/p0-11-protocol-clients/`](../../spikes/p0-11-protocol-clients/)

## 1. 使ったもの

| プロトコル | クライアントの実装 | 相手 | 接続 |
|---|---|---|---|
| LSP | JSON-RPCのフレーミング（`Content-Length`）を自前で実装し、結果の型に `lsp-types` 0.97.0（MIT）を使った | `marksman` 2026-02-08（Markdownの言語サーバー、nixpkgs） | stdio（子プロセス） |
| MCP | 公式のRust SDK `rmcp` 3.9.0（Apache-2.0） | 同じSDKで書いた検証用のサーバー（ツール1つ） | stdio（子プロセス）、Streamable HTTP（ループバック） |
| ACP | 公式のRust SDK `agent-client-protocol` 5.0.0（Apache-2.0） | 同じSDKで書いた検証用のエージェント | stdio（子プロセス） |

- `lsp-types` の最終公開は2024-06で、更新が止まっている。LSPの公式のRustクライアントSDKは無い。
- MCP・ACPの相手は、同じSDKで書いた検証用の実装である。別の実装（他言語のSDK、実在のエージェント）との相互運用は確認していない（npmのパッケージ等の第三者のコードを実行しないため）。

## 2. 結果

### 2.1 LSP（marksman）

| 段階 | 結果 |
|---|---|
| `initialize` | 成立（919 ms。marksmanの起動時間を含む） |
| Capability | `documentSymbol`・`definition`・`completion`・`hover`・`rename` を提供。`positionEncoding` は返されず、LSPの既定のUTF-16になる（クライアントは `utf-8`・`utf-16` を提示した） |
| `textDocument/didOpen` → `textDocument/documentSymbol` | 成立（380 ms）。`# 見出し` の下に `## 小見出し 🇯🇵` の入れ子の記号が返った |
| `shutdown` → `exit` | 正常終了 |

### 2.2 MCP

| 段階 | stdio | Streamable HTTP |
|---|---|---|
| 初期化 | 3.2 ms | 3.8 ms |
| 交換されたプロトコルの版 | `2025-11-25` | `2025-11-25` |
| サーバーのCapability | tools: あり、resources・prompts: なし | 同左 |
| `tools/list` | `word_count` | 同左 |
| `tools/call` | `words=6 chars=25`（0.99 ms） | 同左（2.05 ms） |

### 2.3 ACP

| 段階 | 結果 |
|---|---|
| `initialize` | 成立（4.3 ms）。プロトコルの版 1、エージェントの情報とCapability（`load_session`、プロンプトの画像・音声・埋め込み文脈、MCPの接続方式など）を受け取った |
| `session/new` | 成立（セッションID） |
| `session/prompt` | `session/update`（エージェントのメッセージ `echo: こんにちは Hisui`）の通知を受け、`stop_reason: EndTurn` で応答した（1.01 ms） |

## 3. 気づいた点

- **ACPのSDKの例は、権限の要求を自動で許可する**（`yolo_one_shot_client`）。検証用のクライアントでは、ADR-0008に従い、権限の要求を既定で拒否（`Cancelled`）した。製品でも、ACPエージェント・MCPサーバーからの要求（ACPの権限要求・ファイルの読み書き要求、MCPのSampling・Elicitation・Roots）は Permission Broker を通す（ADR-0008 §5）。
- LSPの位置の単位は、サーバーが `positionEncoding` を返さない場合UTF-16になる。ADR-0003のUTF-16位置の変換を使う。
- `rmcp` の Streamable HTTP のサーバー・クライアントは `axum`・`reqwest` に依存する。MCPの接続モジュール（ADR-0002の分割基準1）の依存として扱う。

## 4. 未確認の事項

- 別の実装（他言語のSDK、実在のMCPサーバー・ACPエージェント）との相互運用。
- LSPのリモート接続（統合開発計画 §9の「ローカル・リモート接続」）。
- モバイルでの接続（Androidでの子プロセスの起動の可否はP0-12で扱う）。
- 認証（MCPのOAuth、ACPの認証）。
