# P0-05: Android向けGPUIバックエンドのプロトタイプ

- **調査日:** 2026-11-08
- **状態:** 完了。完了条件（矩形とテキストの描画、タッチ、IME経由の文字入力）を実機で確認した。**Go（2026-11-09、利用者の判断）。**
- **目的:** Zed本体に無いAndroid向けのGPUIバックエンドを試作し、Go/No-Goの判断と工数の見積もりの材料を得る（実行計画 P0-05）。
- **確認プログラム:** [`spikes/p0-05-gpui-android/`](../../spikes/p0-05-gpui-android/)

## 1. 構成

| 部分 | 方式 | 行数 |
|---|---|---|
| アクティビティ・イベントループ | `android-activity` 0.6.1。NativeActivity版（Javaのコードなし）とGameActivity版（`androidx.games:games-activity` 4.4.0）をcargo featureで切り替える | — |
| `Platform` | iOSの実装（`gpui_ios`）を雛形にした。IMEの状態の変換を含む。メニュー・ファイル選択・クリップボード等は空の実装 | 397 |
| `PlatformWindow` | Linuxのヘッドレスのウィンドウ（`gpui_linux`）を雛形にし、`gpui_wgpu` の描画面とコールバックを加えた | 258 |
| `PlatformDispatcher` | メインスレッドのキュー（`AndroidAppWaker` でイベントループを起こす）、バックグラウンドのスレッド、タイマー | 137 |
| 描画 | `gpui_wgpu`（wgpu。AndroidではVulkanとGLESが既定で有効） | 流用 |
| 文字の整形 | `gpui_wgpu` の `CosmicTextSystem`。`/system/fonts` のRoboto・Noto CJK・Noto Color Emoji・Noto Naskh Arabicを読み込む | 流用 |
| デモ | P0-02の入力の例（Zedの `input.rs`）に、日本語・アラビア語の文字列、矩形、タップ数の表示を加えた | — |

- **GPUI本体（`gpui`・`gpui_wgpu`）は変更せずにAndroid向けにビルドできた**（ADR-0001の参照方式のまま）。
- GPUIには、iOS向けに追加された「タッチの生の入力 → タップ・スクロール等への変換」（`PlatformInput::Touch` とジェスチャー認識）があり、Androidのタッチをそのまま渡すだけでクリックとして扱われた。
- **APKの作成:** NativeActivity版は `aapt2`・`zipalign`・`apksigner` で作成した（[`build-apk.sh`](../../spikes/p0-05-gpui-android/build-apk.sh)、Gradleなし、約20 MB）。GameActivity版はGradle 8.14.6・Android Gradle Plugin 8.13.0で作成した（[`build-gradle-apk.sh`](../../spikes/p0-05-gpui-android/build-gradle-apk.sh)、約22 MB）。AndroidXの依存が古い `kotlin-stdlib-jdk7`・`jdk8` を持ち込みクラスが重複したため、`kotlin-bom` で版を揃えた。

## 2. 環境

- `nix develop .#android`（ADR-0010の改訂。Gradleを追加）。
- エミュレーター: `hisui-api35`（Android 15、x86_64、`-gpu swiftshader_indirect`、ウィンドウなし）。
- 実機: realme RMX6688（Android 17、SDK 37、arm64-v8a、MediaTek MT6991、Mali-G925-Immortalis MC12）。日本語IME `com.kazumaproject.markdownhelperkeyboard`。
- 表示は `adb exec-out screencap` で端末の画面を取得して確認した（検証のアプリが前面にある間のみ）。

## 3. 結果（NativeActivity版）

| 項目 | エミュレーター | 実機 |
|---|---|---|
| 起動・GPU | Vulkan（SwiftShader、CPUによる描画） | Vulkan（Mali-G925）。1280×2800、倍率2.975 |
| 矩形・文字（英字・日本語・アラビア語）の描画 | 描画できた | 描画できた |
| 絵文字 | **描画されなかった** | **描画されなかった** |
| タッチ | タップがクリックとして処理された | 同左 |
| `adb shell input text` の文字 | 確定として入力欄に入った | **入らなかった**（日本語IMEが有効で、IMEがキーを受け取り、入力接続の無いNativeActivityへ結果を渡せない） |
| 特殊キー（Backspace・左右等） | GPUIのキーバインドで処理された | Backspaceは届いた |
| ソフトキーボード | 表示されなかった | IMEの最小化されたバーのみ |

**NativeActivityでは、IMEが有効な実機で文字入力そのものができない。** GameActivityが必須である。

## 4. 結果（GameActivity版、実機）

利用者が日本語IMEで「にほんご」を入力し、変換・変換中の削除・確定を行った。

| 項目 | 結果 |
|---|---|
| ソフトキーボード | 入力欄のタップで表示された（利用者の目視） |
| 変換中の文字列 | `ｎ` → `に` → `にｈ` → … → `にほんご` → `日本語` と、IMEの変換範囲（`compose_region`）が届き、`replace_and_mark_text_in_range` で変換中の文字列として表示された |
| 変換中の削除 | `にひん` → `にひ` → `に` と、変換中の文字列が短くなった |
| 確定 | 変換範囲が無くなった状態（`compose_region=None`、`日本語`）が届き、`replace_text_in_range("日本語")` で変換中の範囲が確定した（入力欄の内容 `日本語`、変換中の範囲なし） |
| IMEの位置の単位 | IMEの選択範囲・変換範囲はJavaの文字列の位置（UTF-16）として扱い、UTF-8のバイト位置に変換した。`日本語`（3単位 ↔ 9バイト）で整合した |
| 表示 | 変換中の文字列・確定した文字列が入力欄に表示された（利用者の目視） |

IMEの状態（編集中の文字列全体・選択範囲・変換範囲）からGPUIの入力ハンドラーへの変換は、次の方式で行った。

- 変換範囲の前で、まだ確定として渡していない部分を `replace_text_in_range` で確定する。
- 変換範囲の文字列を `replace_and_mark_text_in_range` で変換中として渡す。
- 変換範囲が無くなったら、IMEの編集バッファを空に戻す（次の入力が二重に確定しないように）。

この経路は、P0-02（Linux、Wayland・X11）と同じ入力ハンドラーの呼び出しである。

## 5. 分かった制約と未実装の事項

- **絵文字:** フォント（`NotoColorEmoji.ttf`）は読み込んだが描画されなかった。原因は未調査。
- **表示の領域:** ステータスバー・ナビゲーションバー・IMEの領域（インセット）を考慮していない。内容がステータスバーと重なった。
- **描画面の再作成:** `TerminateWindow`（アプリの切り替え・回転・他の画面の前面化）の後の描画面の再作成を実装していない。
- **描画の頻度:** イベントループが16 msごとに描画を要求している。電池の消費を抑えるには、変更があるときだけ描画する（Choreographer等）方式が必要。
- **IME:** 変換中の文字列の中のカーソル位置（選択範囲）と、候補の位置（`update_ime_position` → IMEへの通知）を渡していない。ハードウェアキーボードとの併用、文書の途中への入力での編集バッファの扱いは未確認。
- 未実装（空の実装）: クリップボード、ファイル選択（Storage Access Framework）、URLを開く、共有、ディスプレイの情報、アプリのライフサイクルの通知、戻るジェスチャー、資格情報の保存（Android Keystore）。いずれもJNI経由のJava APIの呼び出しが必要。
- 性能（描画時間、入力から描画までの遅延、電池）は計測していない（エミュレーターはCPU描画）。

## 6. 工数の見積もり（概算。確度は低い）

根拠: 本プロトタイプは815行で、描画・文字・タッチ・キー入力・IMEの変換入力まで動いた。Zedの `gpui_ios`（実験的と明記）は約3,500行である。

| 作業 | 規模の目安 |
|---|---|
| ライフサイクル（描画面の再作成、回転、バックグラウンド）、インセット、変更時のみの描画 | 中 |
| IMEの完成（変換中のカーソル位置、候補の位置、ハードウェアキーボード、文書編集との統合） | 中 |
| JNI経由の機能（クリップボード、Storage Access Framework、URL、共有、Keystore） | 大 |
| 絵文字の描画の原因調査と修正 | 不明（`gpui_wgpu`・cosmic-textの側の可能性） |
| 実機での性能・電池の確認と最適化 | 中 |
| ビルド・署名・CI（Gradle、nix） | 小 |
| GPUIの更新への追従（`Platform` 等のトレイトの変更） | 継続的 |

iOSの実装と同程度（3,000〜4,000行）の規模になると見込む。GPUI本体の変更を必要とする箇所は、本プロトタイプの範囲では見つかっていない（絵文字は未確定）。

## 7. 評価と判断（2026-11-09にGo）

- **判断: Go（利用者が決定）。** GPUI本体を変更せずに、約800行でAndroidの描画・タッチ・日本語の変換入力まで動いた。描画（wgpu）、文字の整形（cosmic-text）、タッチの解釈（iOS向けのジェスチャー認識）をGPUIからそのまま使える。
- 条件: GameActivity（AndroidX）を使い、APKの作成にGradleを使う。§5の未実装の事項、特にJNI経由の機能とライフサイクルは、Phase 9（Android）の作業として計画する。システムバーの領域への重なり（§5の表示の領域）は、統合開発計画のPhase 9に修正項目として記載した。
