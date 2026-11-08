# P0-01: GPUIの配布形態・ライセンス・デスクトップビルド

- **調査日:** 2026-11-08
- **対象:** Zed `v1.27.0`（コミット `c84af5b99cbc366d675064e52b23664a015b6e35`、2026-11-04）。この時点の `main` は `b5cd310b3b54dfc659fcc06453e6358843dfe216`（2026-11-07）。
- **範囲:** 配布形態、ライセンス、iOSバックエンドの状態、Linuxでのビルドと起動、リサイズ時の描画。Windows・macOSのビルドは**未実施**（この環境はLinuxのみ）。

## 1. 配布形態

| 項目 | 結果 | 根拠 |
|---|---|---|
| crates.ioの `gpui` | 最新は `0.2.2`（2025-10-22公開）。約1年更新されていない | crates.io API |
| Zedリポジトリの `gpui` | `crates/gpui` の版番号は `0.2.2` のまま。プラットフォーム層は `gpui_platform`、`gpui_linux`、`gpui_macos`、`gpui_windows`、`gpui_ios`、`gpui_web`、`gpui_wgpu`、`gpui_apple` などに分割されている（crates.ioには未公開） | `crates/` の一覧 |
| 非公式の再配布 | `gpui-archipelago` がZedのタグからcrates.ioへ再配布している（`gpui-ios-gpui-unofficial` 等） | crates.io API |
| Rustツールチェーン | Zedは `rust-toolchain.toml` で `1.98.1` を指定 | Zedリポジトリ |

**結論:** 現行のGPUIを使うには、Zedリポジトリをgitの特定リビジョンで参照する必要がある。非公式の再配布は第三者のビルドであり、供給経路の信頼性の観点から使用しない。

### 1.1 `[patch]` の引き継ぎ

Zedのワークスペースは `[patch.crates-io]` でフォーク版の依存を差し替えている。git依存として `gpui` を参照する側には、この差し替えが引き継がれない（Cargoの仕様）。GPUIの依存に関係するものは次のとおり。

| crate | 差し替え先 |
|---|---|
| `async-task` | `zed-industries/async-task` rev `015845a1…` |
| `async-process` | `zed-industries/async-process` rev `0b6d6713…` |
| `calloop` | `zed-industries/calloop` rev `3759371f…` |
| `khronos-egl` | `zed-industries/khronos-egl` rev `42214b36…`（EGL呼び出しの誤判定によるpanicの修正） |

Hisui Notesのワークスペースにも同じ `[patch.crates-io]` を記載し、GPUIのリビジョンを更新するたびに照合する必要がある。

## 2. ライセンス

### 2.1 GPUIとZed内部の依存

`gpui_platform` から到達するZedリポジトリ内のクレートは、すべて `Apache-2.0` で、各クレートに `LICENSE-APACHE` がある。

`gpui`、`gpui_platform`、`gpui_linux`、`gpui_macos`、`gpui_windows`、`gpui_ios`、`gpui_apple`、`gpui_web`、`gpui_wgpu`、`gpui_macros`、`gpui_util`、`gpui_shared_string`、`collections`、`derive_refineable`、`refineable`、`http_client`、`scheduler`、`sum_tree`、`ztracing`、`ztracing_macro`

### 2.2 推移的な依存

`cargo tree -p gpui_platform -e normal,build --target all` で解決した523パッケージのライセンス:

| ライセンス式 | 件数（概数） |
|---|---|
| MITとApache-2.0のいずれか（表記揺れを含む） | 約340 |
| MIT | 77 |
| Apache-2.0 | 26 |
| Unicode-3.0 | 18 |
| Zlib・BSD・ISCを含む選択式 | 約30 |
| BSD-3-Clause | 6 |
| MPL-2.0 | 3（`cbindgen`（ビルド時のみ）、`dwrote`（Windows）、`option-ext`） |
| その他の選択式 | 下記 |

コピーレフトのライセンスを含む式は、いずれも選択式で、MITまたはApache-2.0を選べる。

- `r-efi`: MIT OR Apache-2.0 OR LGPL-2.1-or-later
- `self_cell`: Apache-2.0 OR GPL-2.0-only

単独でGPL・LGPL・AGPLのパッケージはない。ライセンス未記載のパッケージはない。

**結論:** ADR-0011（MIT OR Apache-2.0）と両立する。`cargo-deny` の許可リストには、ADR-0011の初期リストに加え、選択式の一部として現れる `Unlicense`、`BSL-1.0`、`CC0-1.0`、`MIT-0`、`0BSD`、`Apache-2.0 WITH LLVM-exception` を扱えるようにする（cargo-denyは選択式のいずれかが許可されていれば通す）。

## 3. iOSバックエンド

- Zed本体に `gpui_ios` が追加された（PR #63068、2026-10-14マージ）。PRの説明では「実験的」「初期対応」とされている。
- 対応済み（PRの記載）: UIKit、Metal描画（`gpui_apple`）、タッチとジェスチャー、セーフエリアとキーボードの領域、`UITextInput` によるネイティブのテキスト入力、シミュレーターでの単体テスト、実機向けのリンク。
- 未対応（PRの記載）: ネイティブの編集メニュー、ネットワーク・TLSの回避策、Returnキーとハードウェアキーボードのショートカットのキーイベント。
- 実機・シミュレーターでの動作確認は未実施（P0-04で行う。macOSが必要）。

統合開発計画 §21.1の「公式gpui_iosを基礎とする」は、Zed本体の `gpui_ios` として成立する。

## 4. Androidバックエンド

Zed本体に `gpui_android` は**ない**。描画は `gpui_wgpu`（wgpu）に共通化されており、AndroidのVulkan描画にwgpuを流用できる可能性がある（P0-05で確認する）。第三者による実装（`gpui-mobile` 等）があるが、未評価。

## 5. Linuxでのビルド

### 5.1 環境

- NixOS、Wayland（コンポジターはniri）、XWaylandあり（`DISPLAY=:0`）。12コア、メモリ15 GB。
- Hisui Notesの `flake.nix` のdevShell（rustc 1.99.0）。Zedが指定する1.98.1ではなく、nixpkgsの1.99.0でビルドした。
- devShellに追加したシステムライブラリ: `pkg-config`、`fontconfig`、`freetype`、`libxkbcommon`、`wayland`、`vulkan-loader`、`libx11`、`libxcb`。実行時に読み込まれるため `LD_LIBRARY_PATH` にも含めた（Zedの `nix/build.nix` を参考に、GPUIのプラットフォーム層に必要なものに絞った）。

### 5.2 結果

| 項目 | 結果 |
|---|---|
| `cargo build -p gpui --example hello_world --example text --example input` | 成功。526クレート、3分20秒（`-j 8`、debug）、最大常駐メモリ約1.4 GB |
| `hello_world` の起動（Wayland） | 起動し、8秒後のタイムアウトまでエラーなく動作した |
| `hello_world` の起動（X11、`WAYLAND_DISPLAY` を空にしてXWayland経由） | 同上 |
| 描画内容 | Wayland上でウィンドウが表示され、背景色（`0x505050`）の描画を確認した。ただし、確認用の画面キャプチャでは描画領域がウィンドウの一部（左上）だけで、残りが黒かった。他のウィンドウと重なっていたため、文字の描画は確認できていない |

### 5.3 リサイズ時の描画（2026-11-08、利用者の目視）

P0-03の確認プログラム（`spikes/p0-02-03-gpui-text` の `render`）をniri上で表示し、利用者が幅の変更、他のウィンドウとの並置・重なり、最大化と復帰を行った。

- 描画は新しい大きさに追従し、背景・文字列の崩れやちらつきは無かった。5.2の黒い領域は再現しなかった。
- **ウィンドウに最小サイズが無く、際限なく縮小できた。** GPUIは `WindowOptions::window_min_size` を持ち、Waylandでは `xdg_toplevel.set_min_size`、X11ではサイズのヒントとして渡す。確認プログラムでは設定していなかった（既定は `None`）。Hisui Notesのウィンドウでは最小サイズを設定する（値はUIのSpecで決める）。niriのタイル配置が最小サイズをどこまで尊重するかは未確認。

### 5.4 要確認

- 文字の描画、日本語の表示 → P0-03で確認済み（[`P0-03-gpui-text-rendering.md`](P0-03-gpui-text-rendering.md)）。
- この環境での確認用の画面キャプチャは、他のウィンドウの内容も写るため、今後は行わない。描画の確認はウィンドウ単位のキャプチャか、利用者の目視で行う。

## 6. 未確認の事項

- Windows・macOSでのビルドと起動（保留。統合開発計画 §1.2）。
- iOSでの実機動作（保留）。
- GPUIのテキスト入力・IME（P0-02）、テキスト描画（P0-03）。
