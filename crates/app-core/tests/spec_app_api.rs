//! `docs/spec/app-api.md` の契約の検証。

use std::sync::Arc;

use app_api::{
    API_VERSION, AppApi, Command, ErrorCode, Event, Query, QueryResponse, STATUS_MESSAGE_MAX_BYTES,
};
use app_core::Core;
use futures_channel::mpsc::TryRecvError;
use platform::{DesktopFileSystem, FileSystem, PlatformServices};

struct FakePlatform;
impl PlatformServices for FakePlatform {
    fn name(&self) -> String {
        "test-os".into()
    }
    fn file_system(&self) -> Arc<dyn FileSystem> {
        Arc::new(DesktopFileSystem)
    }
}

fn core() -> Core {
    Core::new(Arc::new(FakePlatform))
}

fn set(text: &str) -> Command {
    Command::SetStatusMessage { text: text.into() }
}

#[test]
fn app_info_reports_versions_and_platform() {
    let QueryResponse::AppInfo(info) = core().query(Query::AppInfo).unwrap() else {
        panic!("unexpected response")
    };
    assert_eq!(info.name, "Hisui Notes");
    assert_eq!(info.api_version, API_VERSION);
    assert_eq!(info.platform, "test-os");
    assert!(!info.version.is_empty());
}

#[test]
fn query_does_not_change_state() {
    let c = core();
    c.execute(set("a")).unwrap();
    let mut rx = c.subscribe();
    for _ in 0..3 {
        assert_eq!(
            c.query(Query::StatusMessage).unwrap(),
            QueryResponse::StatusMessage("a".into())
        );
    }
    assert_eq!(
        rx.try_recv(),
        Err(TryRecvError::Empty),
        "query must not publish events"
    );
}

#[test]
fn command_updates_state_and_events_are_queued_before_return() {
    let c = core();
    let mut rx = c.subscribe();
    c.execute(set("保存しました 👩‍💻")).unwrap();
    // execute が戻った時点でイベントがキューにある（Spec §4 の事後条件）
    assert_eq!(
        rx.try_recv(),
        Ok(Event::StatusMessageChanged {
            text: "保存しました 👩‍💻".into()
        })
    );
    assert_eq!(
        c.query(Query::StatusMessage).unwrap(),
        QueryResponse::StatusMessage("保存しました 👩‍💻".into())
    );
}

#[test]
fn same_text_is_published_again() {
    let c = core();
    let mut rx = c.subscribe();
    c.execute(set("x")).unwrap();
    c.execute(set("x")).unwrap();
    assert!(rx.try_recv().is_ok());
    assert!(rx.try_recv().is_ok());
}

#[test]
fn status_message_at_limit_is_accepted() {
    let c = core();
    let text = "a".repeat(STATUS_MESSAGE_MAX_BYTES);
    assert!(c.execute(set(&text)).is_ok());
}

#[test]
fn status_message_over_limit_is_rejected_without_side_effects() {
    let c = core();
    c.execute(set("before")).unwrap();
    let mut rx = c.subscribe();
    // 上限ちょうどの文字数でも、マルチバイト文字でバイト数が超える場合は拒否する
    let text = "あ".repeat(STATUS_MESSAGE_MAX_BYTES / 3 + 1);
    let err = c.execute(set(&text)).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert_eq!(
        c.query(Query::StatusMessage).unwrap(),
        QueryResponse::StatusMessage("before".into())
    );
    assert_eq!(
        rx.try_recv(),
        Err(TryRecvError::Empty),
        "failed command must not publish events"
    );
}

#[test]
fn subscribers_receive_only_events_after_subscription_in_order() {
    let c = core();
    c.execute(set("before")).unwrap();
    let mut a = c.subscribe();
    c.execute(set("1")).unwrap();
    let mut b = c.subscribe();
    c.execute(set("2")).unwrap();
    let got_a: Vec<_> = std::iter::from_fn(|| a.try_recv().ok()).collect();
    let got_b: Vec<_> = std::iter::from_fn(|| b.try_recv().ok()).collect();
    assert_eq!(
        got_a,
        vec![
            Event::StatusMessageChanged { text: "1".into() },
            Event::StatusMessageChanged { text: "2".into() }
        ]
    );
    assert_eq!(
        got_b,
        vec![Event::StatusMessageChanged { text: "2".into() }]
    );
}

#[test]
fn dropped_subscriber_does_not_break_publishing() {
    let c = core();
    let dropped = c.subscribe();
    let mut kept = c.subscribe();
    drop(dropped);
    c.execute(set("ok")).unwrap();
    assert_eq!(
        kept.try_recv(),
        Ok(Event::StatusMessageChanged { text: "ok".into() })
    );
}
