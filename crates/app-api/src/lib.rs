//! Hisui Notes のアプリケーションAPI。
//!
//! UI（Frontend）とアプリケーションコア（Core）の間の共通のAPI。仕様は `docs/spec/app-api.md`。
//! この crate の公開型に、GPUI・OS・文書形式に固有の型を含めない（ADR-0002）。

use std::fmt;

use serde::{Deserialize, Serialize};

mod documents;

pub use documents::{
    CompositionState, DocumentId, DocumentInfo, EditCommand, Movement, Selection, TextEdit,
    VaultFiles, ViewId, ViewState,
};

/// APIの版。副版では追加のみを行う（Spec §3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ApiVersion {
    pub major: u32,
    pub minor: u32,
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// 現在のAPIの版。
pub const API_VERSION: ApiVersion = ApiVersion { major: 0, minor: 2 };

/// 状態表示の文字列の上限（UTF-8のバイト数）。
pub const STATUS_MESSAGE_MAX_BYTES: usize = 4096;

/// 状態を変更する要求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Command {
    /// 状態表示の文字列を設定する。
    SetStatusMessage {
        text: String,
    },
    // 版0.2（Spec §5A.2）
    OpenVault {
        path: String,
    },
    OpenDocument {
        path: String,
    },
    CloseDocument {
        document: DocumentId,
    },
    Edit {
        document: DocumentId,
        view: ViewId,
        command: EditCommand,
    },
    SaveDocument {
        document: DocumentId,
        overwrite: bool,
    },
    ReloadDocument {
        document: DocumentId,
    },
}

/// 状態を読む要求。副作用を持たない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Query {
    AppInfo,
    StatusMessage,
    // 版0.2（Spec §5A.4）
    VaultFiles,
    DocumentInfo {
        document: DocumentId,
    },
    ViewState {
        document: DocumentId,
        view: ViewId,
    },
    InputTextForRange {
        document: DocumentId,
        view: ViewId,
        range: std::ops::Range<usize>,
    },
    InputSelectedRange {
        document: DocumentId,
        view: ViewId,
    },
    InputMarkedRange {
        document: DocumentId,
        view: ViewId,
    },
    InputToBuffer {
        document: DocumentId,
        view: ViewId,
        offset: usize,
    },
    BufferToInput {
        document: DocumentId,
        view: ViewId,
        offset: usize,
    },
}

/// Query の応答。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum QueryResponse {
    AppInfo(AppInfo),
    StatusMessage(String),
    // 版0.2
    VaultFiles(VaultFiles),
    DocumentInfo(DocumentInfo),
    ViewState(ViewState),
    Text(String),
    /// 範囲と、逆向きか（`head` が前）
    SelectedRange {
        range: std::ops::Range<usize>,
        reversed: bool,
    },
    MarkedRange(Option<std::ops::Range<usize>>),
    Offset(usize),
}

/// アプリケーションの情報。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub api_version: ApiVersion,
    /// プラットフォームの名前（表示用。OS固有の型は含めない）
    pub platform: String,
}

/// Core で起きた変化の通知。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Event {
    StatusMessageChanged {
        text: String,
    },
    // 版0.2（Spec §5A.3）
    VaultOpened {
        root: String,
    },
    DocumentOpened {
        document: DocumentId,
        view: ViewId,
        path: String,
    },
    DocumentClosed {
        document: DocumentId,
    },
    DocumentChanged {
        document: DocumentId,
        version: u64,
        edits: Vec<TextEdit>,
    },
    ViewStateChanged {
        document: DocumentId,
        view: ViewId,
        state: ViewState,
    },
    DirtyChanged {
        document: DocumentId,
        dirty: bool,
    },
    DocumentSaved {
        document: DocumentId,
    },
}

/// エラーの種類（Spec §6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ErrorCode {
    InvalidArgument,
    NotFound,
    PermissionDenied,
    Unsupported,
    Cancelled,
    Internal,
    // 版0.2
    Conflict,
    InvalidData,
    Io,
}

/// APIの共通のエラー。`message` に文書の内容・シークレットを含めない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

/// 診断情報の重大度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// 警告・情報などの診断（統合開発計画 §3.4）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    /// 発生元（モジュール名など）
    pub source: String,
}

/// イベントの受信側。発行の順に Event を受け取る非同期のストリーム。
pub type EventReceiver = futures_channel::mpsc::UnboundedReceiver<Event>;

/// UI から見た Core（Spec §4）。
pub trait AppApi: Send + Sync {
    /// 状態を変更する。`Ok` の場合、対応する Event は戻る前に全購読者のキューに入っている。
    /// `Err` の場合、状態は変更されず、Event も発行されない。
    fn execute(&self, command: Command) -> Result<(), ApiError>;

    /// 状態を読む。状態を変更しない。
    fn query(&self, query: Query) -> Result<QueryResponse, ApiError>;

    /// 呼び出し以降に発行される Event を受け取る。
    fn subscribe(&self) -> EventReceiver;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_display() {
        assert_eq!(API_VERSION.to_string(), "0.2");
    }

    #[test]
    fn public_types_round_trip_through_serde() {
        let cmd = Command::SetStatusMessage {
            text: "日本語 👩‍💻".into(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), cmd);

        let info = QueryResponse::AppInfo(AppInfo {
            name: "Hisui Notes".into(),
            version: "0.1.0".into(),
            api_version: API_VERSION,
            platform: "linux".into(),
        });
        let json = serde_json::to_string(&info).unwrap();
        assert_eq!(serde_json::from_str::<QueryResponse>(&json).unwrap(), info);

        let err = ApiError::new(ErrorCode::InvalidArgument, "too long");
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(serde_json::from_str::<ApiError>(&json).unwrap(), err);
    }
}

#[cfg(test)]
mod v0_2_tests {
    use super::*;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: T) {
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<T>(&json).unwrap(), v);
    }

    #[test]
    fn version_0_2_types_round_trip_through_serde() {
        let (d, v) = (DocumentId(3), ViewId(0));
        round_trip(Command::Edit {
            document: d,
            view: v,
            command: EditCommand::SetComposition {
                text: "にほ".into(),
                selected: Some(0..3),
            },
        });
        round_trip(Command::Edit {
            document: d,
            view: v,
            command: EditCommand::SetSelections {
                selections: vec![Selection { anchor: 4, head: 1 }],
                primary: 0,
            },
        });
        round_trip(Query::InputTextForRange {
            document: d,
            view: v,
            range: 1..5,
        });
        round_trip(Event::ViewStateChanged {
            document: d,
            view: v,
            state: ViewState {
                selections: vec![Selection { anchor: 0, head: 0 }],
                primary: 0,
                composition: Some(CompositionState {
                    range: 0..0,
                    text: "日".into(),
                    selected: 3..3,
                }),
            },
        });
        round_trip(Event::DocumentChanged {
            document: d,
            version: 9,
            edits: vec![TextEdit {
                start: 1,
                old_len: 2,
                new_len: 3,
            }],
        });
        round_trip(QueryResponse::VaultFiles(VaultFiles {
            files: vec!["a/b.md".into()],
            skipped: 1,
        }));
        round_trip(QueryResponse::SelectedRange {
            range: 2..2,
            reversed: true,
        });
        round_trip(ApiError::new(ErrorCode::Conflict, "n.md: changed"));
    }
}
