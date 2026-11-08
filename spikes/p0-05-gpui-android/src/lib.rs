//! P0-05: Android向けGPUIバックエンドのプロトタイプ。製品コードではない。
//!
//! NativeActivity（android-activity）上で、GPUIの Platform・PlatformWindow・PlatformDispatcher を実装し、
//! 描画は gpui_wgpu（Vulkan／GLES）、文字の整形は gpui_wgpu の CosmicTextSystem を使う。

mod demo;
mod dispatcher;
mod platform;
mod window;

use std::rc::Rc;

use android_activity::AndroidApp;

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("hisui"));
    std::panic::set_hook(Box::new(|info| log::error!("panic: {info}")));
    log::info!("android_main start");
    let platform = Rc::new(platform::AndroidPlatform::new(app));
    demo::run(gpui::Application::with_platform(platform));
    log::info!("android_main end");
}
