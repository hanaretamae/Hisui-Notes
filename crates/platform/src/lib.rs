//! OS機能の抽象インターフェースと、デスクトップの実装（ADR-0002）。
//!
//! Core はこのトレイトを通して OS の機能を使う。M1 ではプラットフォームの識別のみを持つ。

/// Core が使う OS の機能。
pub trait PlatformServices: Send + Sync {
    /// プラットフォームの名前（表示用）。
    fn name(&self) -> String;
}

/// デスクトップ（Linux・Windows・macOS）の実装。
#[derive(Debug, Default)]
pub struct DesktopPlatform;

impl PlatformServices for DesktopPlatform {
    fn name(&self) -> String {
        std::env::consts::OS.to_string()
    }
}
