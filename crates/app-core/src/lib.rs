//! アプリケーションコア。`app-api` の `AppApi` を実装する（`docs/spec/app-api.md`）。
//!
//! GPUI に依存しない（ADR-0002）。

mod documents;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use app_api::{
    API_VERSION, ApiError, AppApi, AppInfo, Command, DocumentId, ErrorCode, Event, EventReceiver,
    Query, QueryResponse, STATUS_MESSAGE_MAX_BYTES,
};
use futures_channel::mpsc::{UnboundedSender, unbounded};
use platform::PlatformServices;

pub use document::Snapshot as TextSnapshot;

use documents::OpenDocument;

/// 同じプロセスの UI 向けの API（Spec §5A.5）。文書の内容を複製せずに読む。
pub trait LocalAppApi: AppApi {
    /// 文書のその時点の不変のスナップショット。文書が無い場合は `None`。
    fn document_snapshot(&self, document: DocumentId) -> Option<TextSnapshot>;
}

struct State {
    status_message: String,
    subscribers: Vec<UnboundedSender<Event>>,
    vault: Option<vault::Vault>,
    documents: BTreeMap<DocumentId, OpenDocument>,
    next_document: u64,
}

impl State {
    /// 全購読者へ発行する。受信側を破棄した購読者は取り除く（Spec §4 の不変条件）。
    fn publish(&mut self, event: Event) {
        self.subscribers
            .retain(|tx| tx.unbounded_send(event.clone()).is_ok());
    }

    fn publish_all(&mut self, events: Vec<Event>) {
        for e in events {
            self.publish(e);
        }
    }
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
                vault: None,
                documents: BTreeMap::new(),
                next_document: 0,
            }),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>, ApiError> {
        self.state
            .lock()
            .map_err(|_| ApiError::new(ErrorCode::Internal, "state lock poisoned"))
    }
}

impl AppApi for Core {
    fn execute(&self, command: Command) -> Result<(), ApiError> {
        // 状態の変更と発行を同じロックの中で行い、発行の順序を全購読者で揃える
        let mut state = self.lock()?;
        let events = match command {
            Command::SetStatusMessage { text } => {
                if text.len() > STATUS_MESSAGE_MAX_BYTES {
                    return Err(ApiError::new(
                        ErrorCode::InvalidArgument,
                        format!("status message exceeds {STATUS_MESSAGE_MAX_BYTES} bytes"),
                    ));
                }
                state.status_message = text.clone();
                vec![Event::StatusMessageChanged { text }]
            }
            Command::OpenVault { path } => {
                documents::open_vault(&mut state, &self.platform, &path)?
            }
            Command::OpenDocument { path } => documents::open_document(&mut state, &path)?,
            Command::CloseDocument { document } => documents::close_document(&mut state, document)?,
            Command::Edit {
                document,
                view,
                command,
            } => documents::edit(&mut state, document, view, command)?,
            Command::SaveDocument {
                document,
                overwrite,
            } => documents::save(&mut state, document, overwrite)?,
            Command::ReloadDocument { document } => documents::reload(&mut state, document)?,
            _ => return Err(ApiError::new(ErrorCode::Unsupported, "unsupported command")),
        };
        state.publish_all(events);
        Ok(())
    }

    fn query(&self, query: Query) -> Result<QueryResponse, ApiError> {
        let state = self.lock()?;
        match query {
            Query::AppInfo => Ok(QueryResponse::AppInfo(AppInfo {
                name: "Hisui Notes".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                api_version: API_VERSION,
                platform: self.platform.name(),
            })),
            Query::StatusMessage => Ok(QueryResponse::StatusMessage(state.status_message.clone())),
            q => documents::query(&state, q),
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

impl LocalAppApi for Core {
    fn document_snapshot(&self, document: DocumentId) -> Option<TextSnapshot> {
        let state = self.state.lock().ok()?;
        state
            .documents
            .get(&document)
            .map(|d| d.session.text().clone())
    }
}
