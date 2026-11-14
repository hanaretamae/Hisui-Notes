//! OS機能の抽象インターフェースと、デスクトップの実装（ADR-0002）。
//!
//! Core・Vault はこのトレイトを通して OS の機能を使う。

mod font;
mod fs;

pub use font::system_ui_font_family;
pub use fs::{DesktopFileSystem, DirEntry, EntryKind, FileSystem};

use std::sync::Arc;

/// Core が使う OS の機能。
pub trait PlatformServices: Send + Sync {
    /// プラットフォームの名前（表示用）。
    fn name(&self) -> String;

    /// ファイルシステム（Vault の読み書きに使う）。
    fn file_system(&self) -> Arc<dyn FileSystem>;
}

/// デスクトップ（Linux・Windows・macOS）の実装。
#[derive(Debug, Default)]
pub struct DesktopPlatform;

impl PlatformServices for DesktopPlatform {
    fn name(&self) -> String {
        std::env::consts::OS.to_string()
    }

    fn file_system(&self) -> Arc<dyn FileSystem> {
        Arc::new(DesktopFileSystem)
    }
}
