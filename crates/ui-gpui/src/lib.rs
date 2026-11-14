//! GPUI のフロントエンド。Core とは `app-api` の `AppApi` だけを通して連携する（ADR-0002）。

use std::sync::Arc;

use app_api::{ApiError, AppApi, AppInfo, Command, ErrorCode, Event, Query, QueryResponse};
use futures::StreamExt as _;
use gpui::{
    Context, IntoElement, ParentElement, Render, SharedString, Styled, Task, Window, div,
    prelude::*, rgb,
};

/// 最初の画面。アプリの情報と状態表示を表示する。
pub struct MainView {
    api: Arc<dyn AppApi>,
    info: AppInfo,
    status: SharedString,
    pings: u32,
    _events: Task<()>,
}

impl MainView {
    /// Query で初期の状態を取得し、Event の購読を始める。
    pub fn new(api: Arc<dyn AppApi>, cx: &mut Context<Self>) -> Result<Self, ApiError> {
        let info = match api.query(Query::AppInfo)? {
            QueryResponse::AppInfo(info) => info,
            _ => {
                return Err(ApiError::new(
                    ErrorCode::Internal,
                    "unexpected response to AppInfo",
                ));
            }
        };
        let status = match api.query(Query::StatusMessage)? {
            QueryResponse::StatusMessage(s) => s,
            _ => {
                return Err(ApiError::new(
                    ErrorCode::Internal,
                    "unexpected response to StatusMessage",
                ));
            }
        };
        let mut events = api.subscribe();
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this.update(cx, |view, cx| view.apply(event, cx)).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            api,
            info,
            status: status.into(),
            pings: 0,
            _events: task,
        })
    }

    /// Event を表示に反映する。表示の状態は Event からのみ変わる。
    fn apply(&mut self, event: Event, cx: &mut Context<Self>) {
        // 未対応の Event（副版で追加されたもの）は無視する
        if let Event::StatusMessageChanged { text } = event {
            self.status = text.into();
            cx.notify();
        }
    }

    /// 表示中の状態表示の文字列。
    pub fn status(&self) -> &str {
        &self.status
    }

    /// 表示中の見出し（アプリ名と版）。
    pub fn title(&self) -> String {
        format!("{} {}", self.info.name, self.info.version)
    }

    fn ping(&mut self) {
        self.pings += 1;
        let text = format!("Ping {}", self.pings);
        if let Err(e) = self.api.execute(Command::SetStatusMessage { text }) {
            self.status = format!("error: {e}").into();
        }
    }
}

impl Render for MainView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x1f2328))
            .child(div().text_xl().child(self.title()))
            .child(format!(
                "API {} · {}",
                self.info.api_version, self.info.platform
            ))
            .child(
                div()
                    .id("status")
                    .debug_selector(|| "status".into())
                    .child(self.status.clone()),
            )
            .child(
                div()
                    .id("ping")
                    .debug_selector(|| "ping".into())
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(rgb(0x1f2328))
                    .rounded_md()
                    .cursor_pointer()
                    .child("Ping")
                    .on_click(cx.listener(|this, _, _, _cx| this.ping())),
            )
    }
}
