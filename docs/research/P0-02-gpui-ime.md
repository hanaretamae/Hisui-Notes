# P0-02: GPUIのテキスト入力とIME（Linux）

- **調査日:** 2026-11-08
- **状態:** Wayland・X11（XWayland）で確認済み。X11では、GPUIのXIMの初期化に競合があり、失敗すると日本語入力ができない。Windows・macOSは保留（統合開発計画 §1.2）。
- **対象:** Zed `v1.27.0`（コミット `c84af5b9`、ADR-0001）の `gpui`・`gpui_platform`（機能 `wayland`・`x11`）。
- **確認プログラム:** [`spikes/p0-02-03-gpui-text/`](../../spikes/p0-02-03-gpui-text/) の `ime`（Zedの `crates/gpui/examples/input.rs` を複製し、入力ハンドラーの呼び出しを記録するようにしたもの）

## 1. 環境

- NixOS、niri 26.04（Wayland）、XWaylandあり。
- fcitx5 5.1.23（`XMODIFIERS=@im=fcitx`）。IBusは導入されていないため確認していない。
- GPUIのLinuxでのIMEの実装: Waylandは `zwp_text_input_v3`、X11はXIM（`zed-xim`）。

## 2. 方法

単一行の入力欄で、利用者がfcitx5により「にほんご」を入力し、変換・確定・変換中の削除を行った。`EntityInputHandler` の各メソッドの呼び出し（引数と呼び出し後の内容）を記録した。表示（変換中の文字列、候補ウィンドウの位置）は利用者が目視で確認した。

## 3. 結果（Wayland）

| 項目 | 結果 |
|---|---|
| 変換中の文字列と確定した文字列の区別 | **できる。** 変換中は `replace_and_mark_text_in_range`（変換中の範囲が設定される）、確定は `replace_text_in_range`（変換中の範囲が解除される）で届いた |
| 変換中の文字列の更新 | `n` → `に` → `にh` → `にほ` → … → `日本語` と、変換中の範囲全体を置き換える形で届いた |
| 確定 | `replace_text_in_range(None, "日本語")` で変換中の範囲が確定した |
| 変換中の削除 | 1文字ずつ短くなる変換中の文字列が届き、最後は空文字列での `replace_text_in_range(Some(3..4), "")` で変換中の範囲が消えた |
| 候補ウィンドウの位置 | 入力欄の近くに表示された（利用者の目視）。GPUIは `bounds_for_range` で位置を問い合わせる |
| 表示 | 問題なし（利用者の目視） |

### 3.1 制約

- **変換中の文字列の中のカーソル位置と、文節の情報が渡されない。** `new_selected_range_utf16` は常に `None` だった。`gpui_linux` は、Waylandの `PreeditString` の `cursor_begin`・`cursor_end` を捨てている（`wayland/client.rs`）。X11でも、XIMの変換中の文字列の属性（`feedbacks`）を使っていない（`x11/xim_handler.rs`）。このため、変換中の文節の強調や、変換中の文字列の中でのカーソル位置を表示できない。対応にはGPUIの変更（上流への提案、またはフォーク。ADR-0001 Alternatives 3）が必要。
- 同じ変換中の文字列が2〜3回続けて届くことがある（再描画が増えるが、内容は同じ）。
- 変換の途中で一瞬だけ異なる文字列（`2本g`）が届いた。fcitx5側の逐次変換の途中結果と考えられる。編集側は最後に届いた変換中の文字列を表示すればよい。

### 3.2 X11（XWayland）の結果

| 項目 | 結果 |
|---|---|
| XIMの初期化 | **起動時に競合がある。** XIMの接続が完了する前に、GPUIが無効なID（`input_method_id: 0`）で入力コンテキストの作成（`CreateIc`）を送ることがある。この場合fcitxは応答せず、その起動の間はキーが1つもfcitxへ転送されない（`ForwardEvent` が0回）。6回の起動のうち2回で発生した（`RUST_LOG=zed_xim=trace` のトレースで確認） |
| 初期化に失敗した場合 | キーが変換されずにそのまま英字として確定する。どのキーでも入力方式を切り替えられない（利用者が確認） |
| 初期化に成功した場合 | 「半角/全角」キーまたは「無変換」キーで日本語入力に切り替わり、`日本語` が確定した（`replace_text_in_range(None, "日本語")`）。キーはfcitxへ転送された（`ForwardEvent` 50回） |
| 変換中の文字列の表示 | **アプリ内には表示されない（インライン入力にならない）。** GPUIは変換中の文字列をアプリ側で描画する方式（`PREEDIT_CALLBACKS`）を要求しているが、fcitxから変換中の文字列（`PreeditDraw`）は1度も届かなかった。fcitx5のXIMフロントエンドの設定「Use On The Spot Style」が無効（利用者の設定ファイルなし）のためと考えられる（未検証。設定の変更にはfcitx5の再起動が必要） |
| XIMの言語設定 | 環境は `ja_JP.UTF-8` だが、GPUIはXIMを `C` で開いている（`gpui_linux` の `x11/xim_handler.rs` で `client.open("C")`。`zed-xim` にも「TODO: set locale」が残る）。今回の入力には影響しなかった |
| CapsLockによる切り替え | XWaylandのキー配置ではCapsLockが `Eisu_toggle` で、fcitx5の切り替えキー（`Zenkaku_Hankaku`、`Muhenkan`）に該当しない。Waylandでは切り替えられたCapsLockが、X11では効かない。環境のキー配置の違いで、GPUIの問題ではない |

X11での日本語入力は、GPUIのXIMの初期化の競合を直さない限り、起動ごとに失敗する可能性がある。Linuxのデスクトップは、Waylandを主な対象とし、X11（XWayland）を従とすることを提案する。

### 3.3 編集機能の設計への影響（提案。Specで決める）

- 変換中の文字列は文書の内容とは別に保持し、確定するまで文書（テキストバッファ・編集履歴）に入れない設計にする（ADR-0003の編集履歴に変換中の更新を記録しない）。今回のGPUIの例は変換中の文字列を内容に直接書き込んでいるが、Hisui Notesではこの方式を採らないことを提案する。
- 位置はUTF-16で受け渡される。ADR-0003のUTF-16位置の変換を使う。

## 4. 未確認の事項

- GPUIのXIMの初期化の競合の原因箇所の特定。上流へは報告しない（2026-11-08、利用者の判断）。
- fcitx5の「Use On The Spot Style」を有効にした場合のX11でのインライン入力。
- IBus。
- 複数行の文書での候補ウィンドウの位置（今回は単一行の入力欄）。
- 再変換（確定済みの文字列の再変換。`text_for_range` を使う）。
