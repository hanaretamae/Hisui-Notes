//! アプリケーションコア。`app-api` の `AppApi` を実装する（`docs/spec/app-api.md`）。
//!
//! GPUI に依存しない（ADR-0002）。

use std::sync::{Arc, Mutex};

use app_api::{
    API_VERSION, ApiError, AppApi, AppInfo, Command, ErrorCode, Event, EventReceiver, Query,
    QueryResponse, STATUS_MESSAGE_MAX_BYTES,
};
use futures_channel::mpsc::{UnboundedSender, unbounded};
use platform::PlatformServices;

struct State {
    status_message: String,
    subscribers: Vec<UnboundedSender<Event>>,
}

/// アプリケーションコア。
pub struct Core {
    platform: Arc<dyn PlatformServices>,
    state: Mutex<State>,
}

impl Core {
    pub fn new(platform: Arc<dyn PlatformServices>) -> Self {
        Self {
            platform,
            state: Mutex::new(State {
                status_message: String::new(),
                subscribers: Vec::new(),
            }),
        }
    }

    /// 全購読者へ発行する。受信側を破棄した購読者は取り除く（Spec §4 の不変条件）。
    fn publish(state: &mut State, event: Event) {
        state
            .subscribers
            .retain(|tx| tx.unbounded_send(event.clone()).is_ok());
    }
}

impl AppApi for Core {
    fn execute(&self, command: Command) -> Result<(), ApiError> {
        match command {
            Command::SetStatusMessage { text } => {
                if text.len() > STATUS_MESSAGE_MAX_BYTES {
                    return Err(ApiError::new(
                        ErrorCode::InvalidArgument,
                        format!("status message exceeds {STATUS_MESSAGE_MAX_BYTES} bytes"),
                    ));
                }
                // 状態の変更と発行を同じロックの中で行い、発行の順序を全購読者で揃える
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| ApiError::new(ErrorCode::Internal, "state lock poisoned"))?;
                state.status_message = text.clone();
                Self::publish(&mut state, Event::StatusMessageChanged { text });
                log::debug!("status message updated");
                Ok(())
            }
            _ => Err(ApiError::new(ErrorCode::Unsupported, "unsupported command")),
        }
    }

    fn query(&self, query: Query) -> Result<QueryResponse, ApiError> {
        match query {
            Query::AppInfo => Ok(QueryResponse::AppInfo(AppInfo {
                name: "Hisui Notes".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                api_version: API_VERSION,
                platform: self.platform.name(),
            })),
            Query::StatusMessage => {
                let state = self
                    .state
                    .lock()
                    .map_err(|_| ApiError::new(ErrorCode::Internal, "state lock poisoned"))?;
                Ok(QueryResponse::StatusMessage(state.status_message.clone()))
            }
            _ => Err(ApiError::new(ErrorCode::Unsupported, "unsupported query")),
        }
    }

    fn subscribe(&self) -> EventReceiver {
        let (tx, rx) = unbounded();
        if let Ok(mut state) = self.state.lock() {
            state.subscribers.push(tx);
        }
        rx
    }
}
