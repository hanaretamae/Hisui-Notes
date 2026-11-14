//! Hisui Notes の実行ファイル。Core・Platform・Frontend を組み立てる（ADR-0002）。

use std::sync::Arc;

use app_api::AppApi;
use app_core::Core;
use gpui::{App, AppContext as _, Bounds, WindowBounds, WindowOptions, px, size};
use platform::DesktopPlatform;
use ui_gpui::MainView;

/// ウィンドウの最小サイズ（暫定値。UIのSpecで決める。P0-01: 最小サイズが無いと際限なく縮小できる）
const MIN_WINDOW_SIZE: (f32, f32) = (480.0, 320.0);

fn main() {
    env_logger::init();
    let api: Arc<dyn AppApi> = Arc::new(Core::new(Arc::new(DesktopPlatform)));
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(MIN_WINDOW_SIZE.0), px(MIN_WINDOW_SIZE.1))),
            ..Default::default()
        };
        let api = api.clone();
        if let Err(e) = cx.open_window(options, move |_, cx| {
            cx.new(|cx| MainView::new(api, cx).expect("initial queries"))
        }) {
            log::error!("failed to open window: {e:#}");
            cx.quit();
        }
        cx.activate(true);
    });
}
