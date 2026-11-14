//! Hisui Notes の実行ファイル。Core・Platform・Frontend を組み立てる（ADR-0002）。
//!
//! 使い方: `hisui-notes [Vaultのディレクトリ]`（省略した場合は画面から選ぶ）。

use std::sync::Arc;

use app_core::{Core, LocalAppApi};
use gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use platform::DesktopPlatform;
use ui_gpui::Workspace;

/// ウィンドウの最小サイズ（暫定値。UIのSpecで決める。P0-01: 最小サイズが無いと際限なく縮小できる）
const MIN_WINDOW_SIZE: (f32, f32) = (480.0, 320.0);

fn main() {
    env_logger::init();
    let vault_arg = std::env::args_os().nth(1).map(|a| {
        let path = std::path::absolute(&a).unwrap_or_else(|_| a.into());
        path.to_string_lossy().into_owned()
    });
    let api: Arc<dyn LocalAppApi> = Arc::new(Core::new(Arc::new(DesktopPlatform)));
    gpui_platform::application().run(move |cx: &mut App| {
        ui_gpui::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1000.0), px(700.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(MIN_WINDOW_SIZE.0), px(MIN_WINDOW_SIZE.1))),
            ..Default::default()
        };
        let api = api.clone();
        let window = cx.open_window(options, move |window, cx| {
            cx.new(|cx| Workspace::new(api, window, cx).expect("initial queries"))
        });
        match window {
            Ok(window) => {
                if let Some(path) = vault_arg.clone() {
                    let _ =
                        window.update(cx, |ws, window, cx| ws.open_vault_path(path, window, cx));
                }
            }
            Err(e) => {
                log::error!("failed to open window: {e:#}");
                cx.quit();
            }
        }
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
