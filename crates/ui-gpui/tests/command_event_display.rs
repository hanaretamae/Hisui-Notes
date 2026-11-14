//! M1 の完了条件: ui-gpui が Command を送り、Event を受信して表示する（実行計画 §6.3）。

use std::sync::Arc;

use app_api::{API_VERSION, AppApi, Command};
use app_core::Core;
use gpui::{Modifiers, TestAppContext};
use platform::DesktopPlatform;
use ui_gpui::MainView;

fn core() -> Arc<dyn AppApi> {
    Arc::new(Core::new(Arc::new(DesktopPlatform)))
}

#[gpui::test]
fn shows_app_info_from_query(cx: &mut TestAppContext) {
    let api = core();
    let (view, _cx) = cx.add_window_view(|_, cx| MainView::new(api, cx).unwrap());
    view.read_with(cx, |v, _| {
        assert!(v.title().starts_with("Hisui Notes "));
        assert_eq!(v.status(), "");
    });
    // 表示に使う版が API の版と一致する
    assert_eq!(API_VERSION.to_string(), "0.1");
}

#[gpui::test]
fn click_sends_command_and_event_updates_display(cx: &mut TestAppContext) {
    let api = core();
    let (view, vcx) = cx.add_window_view(|_, cx| MainView::new(api, cx).unwrap());
    vcx.run_until_parked();
    let ping = vcx.debug_bounds("ping").expect("ping button is rendered");
    vcx.simulate_click(ping.center(), Modifiers::none());
    vcx.run_until_parked();
    view.read_with(vcx, |v, _| assert_eq!(v.status(), "Ping 1"));
    vcx.simulate_click(ping.center(), Modifiers::none());
    vcx.run_until_parked();
    view.read_with(vcx, |v, _| assert_eq!(v.status(), "Ping 2"));
}

#[gpui::test]
fn display_follows_events_from_other_senders(cx: &mut TestAppContext) {
    // 表示は Event からのみ変わる: UI 以外から送られた Command の結果も表示される
    let api = core();
    let view_api = api.clone();
    let (view, vcx) = cx.add_window_view(|_, cx| MainView::new(view_api, cx).unwrap());
    api.execute(Command::SetStatusMessage {
        text: "外部から 👩‍💻".into(),
    })
    .unwrap();
    vcx.run_until_parked();
    view.read_with(vcx, |v, _| assert_eq!(v.status(), "外部から 👩‍💻"));
}
