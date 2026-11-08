# P0-12: Androidのストア規約と実行環境の制約

- **調査日:** 2026-11-09
- **状態:** 完了（文書による調査）。iOSは保留（統合開発計画 §1.2）。
- **目的:** WASMプラグインの配布、LSP・MCPの外部プロセスの実行、リモート接続について、Google Playの規約とAndroidの実行環境の制約を確認し、方式への影響を記録する（実行計画 P0-12）。
- **注意:** 規約の解釈は筆者の判断であり、Googleの審査の判断を保証しない。規約の本文は改訂されうるため、配布の前に再確認する。

## 1. 出典と該当箇所

### 1.1 Google Play「Device and Network Abuse」

出典: <https://support.google.com/googleplay/android-developer/answer/9888379?hl=en>（2026-11-09に取得。更新日の記載なし）

- 実行可能コードのダウンロード: "An app distributed via Google Play may not modify, replace, or update itself using any method other than Google Play's update mechanism. Likewise, an app may not download executable code (such as dex, JAR, .so files) from a source other than Google Play."
- 適用除外: "This restriction does not apply to code that runs in a virtual machine or an interpreter where either provides indirect access to Android APIs (such as JavaScript in a webview or browser)."
- 実行時に読み込むインタプリタ言語: "Apps or third-party code, like SDKs, with interpreted languages (JavaScript, Python, Lua, etc.) loaded at run time (for example, not packaged with the app) must not allow potential violations of Google Play policies."
- プラグイン・拡張機能・アドオンについての記載は無い。

### 1.2 Android 10（API 29）の動作の変更

出典: <https://developer.android.com/about/versions/10/behavior-changes-10>（2026-11-09に取得）

- "Untrusted apps that target Android 10 cannot invoke `execve()` directly on files within the app's home directory."
- "Execution of files from the writable app home directory is a W^X violation. Apps should load only the binary code that's embedded within an app's APK file."
- API 29以上を対象とするアプリに適用される。Hisui Notesの検証（P0-05）は `targetSdk` 35。

### 1.3 平文の通信（HTTP）

出典: <https://developer.android.com/privacy-and-security/security-config>、<https://developer.android.com/guide/topics/manifest/application-element>（2026-11-09に検索結果で確認）

- Android 9（API 28）以降を対象とするアプリでは、平文の通信は既定で無効。許可する場合は Network Security Config でドメインごとに `cleartextTrafficPermitted` を指定する。
- ループバック（localhost）の扱いの詳細は、今回の調査では確認できなかった。

## 2. Hisui Notesへの影響

| 対象 | 制約 | 影響（筆者の解釈・提案） |
|---|---|---|
| WASMプラグイン・パーサー（実行時にダウンロード） | 実行可能コードのダウンロードの禁止。VM・インタプリタで、Android APIへのアクセスが間接的なものは適用除外 | Wasmi（ADR-0005、インタプリタ、JITなし）で実行し、Android APIへのアクセスをホスト関数と Permission Broker（ADR-0006、ADR-0008）経由に限る方式は、適用除外の記述（VM・インタプリタ、間接的なアクセス）に合うと考える。ただし「規約違反を可能にしてはならない」の条項があるため、プラグインの配布元・署名・審査の仕組みと、権限の既定拒否を維持する |
| ネイティブコードのプラグイン（`.so`） | ダウンロードの禁止 | Androidでは提供しない。WASMのみとする（統合開発計画 §19.3の方針と一致） |
| LSPサーバーの外部プロセス | アプリのホームディレクトリのファイルを `execve()` できない（API 29以上） | 利用者が追加した言語サーバーの実行ファイルは起動できない。Androidでは、(1) リモートのLSPへの接続、(2) APKに同梱した言語サーバー（固定の組み合わせ）、(3) WASMで動く言語サーバー、のいずれかになる。統合開発計画 §19.3の「ネイティブプロセス・リモート接続」のうち、Androidではリモート接続が主になる |
| MCPサーバー（stdio） | 同上 | AndroidではstdioのMCPサーバーを起動できない。Streamable HTTP（P0-11）による接続を使う |
| リモート接続（LSP・MCP・ACP） | 平文の通信は既定で無効 | HTTPSを使う。平文のHTTPはループバックに限るADR-0008 §2の方針と一致する（Androidでのループバックの扱いは未確認） |
| Hisui Notes本体の更新 | Google Play以外の方法での自己更新の禁止 | Google Playで配布する版は、本体の更新をGoogle Playに任せる |

## 3. 参考: 第三者のAndroid向けGPUI

検索の過程で、第三者によるZedのAndroid移植（`Dylanmurzello/zed-android-port`、`crates/gpui_android`）があることを確認した。内容・ライセンスは評価していない（ADR-0001の方針により、第三者の再配布は使わない）。P0-05のGo（独自の実装）の判断は変えない。

## 4. 未確認の事項

- プラグインの配布の仕組み（配布元、署名、審査）がGoogle Playの審査でどう扱われるか。実際の申請の前に確認する。
- Android以外のストア（F-Droid等）での配布の要件。
- AndroidのNetwork Security Configでのループバックの扱い。
- APKに同梱した実行ファイル（ネイティブライブラリの領域）の起動の可否（第三者の報告はあるが、公式の資料では確認していない）。
