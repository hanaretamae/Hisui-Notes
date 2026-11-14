//! Hisui Notes のアプリケーションAPI。
//!
//! UI（Frontend）とアプリケーションコア（Core）の間の共通のAPI。仕様は `docs/spec/app-api.md`。
//! この crate の公開型に、GPUI・OS・文書形式に固有の型を含めない（ADR-0002）。

use std::fmt;

use serde::{Deserialize, Serialize};

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
pub const API_VERSION: ApiVersion = ApiVersion { major: 0, minor: 1 };

/// 状態表示の文字列の上限（UTF-8のバイト数）。
pub const STATUS_MESSAGE_MAX_BYTES: usize = 4096;

/// 状態を変更する要求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Command {
    /// 状態表示の文字列を設定する。
    SetStatusMessage { text: String },
}

/// 状態を読む要求。副作用を持たない。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Query {
    AppInfo,
    StatusMessage,
}

/// Query の応答。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum QueryResponse {
    AppInfo(AppInfo),
    StatusMessage(String),
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
    StatusMessageChanged { text: String },
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
        assert_eq!(API_VERSION.to_string(), "0.1");
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
