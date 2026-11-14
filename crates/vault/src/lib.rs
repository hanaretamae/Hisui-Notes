//! Vault、ファイル入出力、外部変更検出、リンク索引（ADR-0002）。
//!
//! 現在は Vault を開く・一覧・読み込み・保存（`docs/spec/vault.md`）。外部変更の監視とリンク索引は後続の版で追加する。

mod path;

use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use platform::{EntryKind, FileSystem};

pub use path::{InvalidPathReason, VaultPath};

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// ファイルの状態（Spec §5）。保存時の衝突の検出に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileState {
    /// 先頭に BOM があったか（保存時に付け直す）。
    pub bom: bool,
    /// 内容（BOM を含むバイト列）の長さ。
    pub len: u64,
    /// 内容のハッシュ値（FNV-1a 64 bit）。
    pub hash: u64,
}

impl FileState {
    fn of(bytes: &[u8]) -> Self {
        Self {
            bom: bytes.starts_with(BOM),
            len: bytes.len() as u64,
            hash: fnv1a(bytes),
        }
    }

    fn same_content(&self, other: &FileState) -> bool {
        self.len == other.len && self.hash == other.hash
    }
}

/// FNV-1a（64 bit）。版をまたいで同じ値になる単純なハッシュ。偶発的な変更の検出に使い、改ざんの検出には使わない。
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// 読み込んだテキストファイル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedFile {
    /// BOM を除いた内容。
    pub text: String,
    pub state: FileState,
}

/// 一覧から除外した要素（Spec §4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedEntry {
    /// 表示用のパス（UTF-8 でない名前は置き換え文字を含む）。
    pub display_path: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// 名前が UTF-8 でない。
    NonUtf8Name,
    /// 名前が `VaultPath` の要素の条件を満たさない。
    InvalidName(InvalidPathReason),
    /// 読めなかったディレクトリ。
    Unreadable(io::ErrorKind),
}

/// 一覧の結果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listing {
    /// 通常のファイル（`VaultPath` の昇順）。
    pub files: Vec<VaultPath>,
    pub skipped: Vec<SkippedEntry>,
}

/// 保存時の衝突の種類（Spec §6.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    Modified,
    Deleted,
}

/// Vault の操作の誤り（Spec §7）。文書の内容を含めない。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VaultError {
    InvalidPath {
        path: String,
        reason: InvalidPathReason,
    },
    NotFound {
        path: VaultPath,
    },
    NotAFile {
        path: VaultPath,
    },
    Symlink {
        path: VaultPath,
    },
    NotUtf8 {
        path: VaultPath,
        valid_up_to: usize,
    },
    Conflict {
        path: VaultPath,
        kind: ConflictKind,
    },
    /// Vault の根が開けない（存在しない、ディレクトリでない）。
    InvalidRoot {
        kind: io::ErrorKind,
    },
    Io {
        path: Option<VaultPath>,
        kind: io::ErrorKind,
        message: String,
    },
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath { path, reason } => {
                write!(f, "invalid vault path {path:?}: {reason}")
            }
            Self::NotFound { path } => write!(f, "{path}: not found"),
            Self::NotAFile { path } => write!(f, "{path}: not a regular file"),
            Self::Symlink { path } => write!(f, "{path}: symbolic links are not opened"),
            Self::NotUtf8 { path, valid_up_to } => {
                write!(f, "{path}: not valid UTF-8 (at byte {valid_up_to})")
            }
            Self::Conflict { path, kind } => {
                write!(f, "{path}: changed outside the app ({kind:?})")
            }
            Self::InvalidRoot { kind } => write!(f, "vault root cannot be opened: {kind}"),
            Self::Io {
                path: Some(p),
                kind,
                message,
            } => write!(f, "{p}: {kind}: {message}"),
            Self::Io {
                path: None,
                kind,
                message,
            } => write!(f, "{kind}: {message}"),
        }
    }
}

impl std::error::Error for VaultError {}

fn io_err(path: &VaultPath, e: io::Error) -> VaultError {
    match e.kind() {
        io::ErrorKind::NotFound => VaultError::NotFound { path: path.clone() },
        kind => VaultError::Io {
            path: Some(path.clone()),
            kind,
            message: e.to_string(),
        },
    }
}

/// 開いた Vault。
pub struct Vault {
    root: PathBuf,
    fs: Arc<dyn FileSystem>,
}

impl fmt::Debug for Vault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Vault")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// `root`（OS の絶対パス）を Vault として開く。ディレクトリでなければエラー（Spec §4）。
    pub fn open(root: impl Into<PathBuf>, fs: Arc<dyn FileSystem>) -> Result<Self, VaultError> {
        let root = root.into();
        match fs.kind(&root, true) {
            Ok(EntryKind::Dir) => Ok(Self { root, fs }),
            Ok(_) => Err(VaultError::InvalidRoot {
                kind: io::ErrorKind::NotADirectory,
            }),
            Err(e) => Err(VaultError::InvalidRoot { kind: e.kind() }),
        }
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// 通常のファイルを再帰的に列挙する（Spec §4）。
    pub fn list(&self) -> Result<Listing, VaultError> {
        let mut out = Listing::default();
        let mut stack: Vec<Option<VaultPath>> = vec![None];
        while let Some(dir) = stack.pop() {
            let fs_dir = dir
                .as_ref()
                .map_or_else(|| self.root.clone(), |d| d.to_fs_path(&self.root));
            let entries = match self.fs.read_dir(&fs_dir) {
                Ok(e) => e,
                Err(e) if dir.is_none() => return Err(VaultError::InvalidRoot { kind: e.kind() }),
                Err(e) => {
                    let display_path = dir.as_ref().map(VaultPath::to_string).unwrap_or_default();
                    out.skipped.push(SkippedEntry {
                        display_path,
                        reason: SkipReason::Unreadable(e.kind()),
                    });
                    continue;
                }
            };
            for entry in entries {
                let display = |name: &str| match &dir {
                    Some(d) => format!("{d}/{name}"),
                    None => name.to_string(),
                };
                let Some(name) = entry.name.to_str() else {
                    let lossy = entry.name.to_string_lossy();
                    out.skipped.push(SkippedEntry {
                        display_path: display(&lossy),
                        reason: SkipReason::NonUtf8Name,
                    });
                    continue;
                };
                // `.` で始まる名前は含めず、辿らない（利用者の決定）
                if name.starts_with('.') {
                    continue;
                }
                if let Err(reason) = path::check_component(name) {
                    out.skipped.push(SkippedEntry {
                        display_path: display(name),
                        reason: SkipReason::InvalidName(reason),
                    });
                    continue;
                }
                let child = VaultPath::join(dir.as_ref(), name);
                match entry.kind {
                    EntryKind::File => out.files.push(child),
                    EntryKind::Dir => stack.push(Some(child)),
                    // シンボリックリンクとその他の種類は含めない
                    EntryKind::Symlink | EntryKind::Other => {}
                }
            }
        }
        out.files.sort();
        Ok(out)
    }

    /// パスの途中の要素とファイル自体がシンボリックリンクでなく、ファイルが通常のファイルであることを確かめる（Spec §5）。
    fn check_regular_file(&self, path: &VaultPath) -> Result<PathBuf, VaultError> {
        let mut p = self.root.clone();
        let n = path.components().count();
        for (i, c) in path.components().enumerate() {
            p.push(c);
            match self.fs.kind(&p, false).map_err(|e| io_err(path, e))? {
                EntryKind::Symlink => return Err(VaultError::Symlink { path: path.clone() }),
                EntryKind::File if i + 1 == n => {}
                EntryKind::Dir if i + 1 < n => {}
                _ if i + 1 == n => return Err(VaultError::NotAFile { path: path.clone() }),
                _ => return Err(VaultError::NotFound { path: path.clone() }),
            }
        }
        Ok(p)
    }

    /// テキストファイルを読み込む（Spec §5）。
    pub fn load(&self, path: &VaultPath) -> Result<LoadedFile, VaultError> {
        let fs_path = self.check_regular_file(path)?;
        let bytes = self.fs.read(&fs_path).map_err(|e| io_err(path, e))?;
        let state = FileState::of(&bytes);
        let body = if state.bom {
            &bytes[BOM.len()..]
        } else {
            &bytes[..]
        };
        let text = std::str::from_utf8(body)
            .map_err(|e| VaultError::NotUtf8 {
                path: path.clone(),
                valid_up_to: e.valid_up_to(),
            })?
            .to_string();
        Ok(LoadedFile { text, state })
    }

    /// 衝突を検出してから保存する（Spec §6.2）。
    pub fn save(
        &self,
        path: &VaultPath,
        text: &str,
        expected: &FileState,
    ) -> Result<FileState, VaultError> {
        let fs_path = match self.check_regular_file(path) {
            Err(VaultError::NotFound { .. }) => {
                return Err(VaultError::Conflict {
                    path: path.clone(),
                    kind: ConflictKind::Deleted,
                });
            }
            r => r?,
        };
        let current = match self.fs.read(&fs_path) {
            Ok(b) => FileState::of(&b),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(VaultError::Conflict {
                    path: path.clone(),
                    kind: ConflictKind::Deleted,
                });
            }
            Err(e) => return Err(io_err(path, e)),
        };
        if !current.same_content(expected) {
            return Err(VaultError::Conflict {
                path: path.clone(),
                kind: ConflictKind::Modified,
            });
        }
        self.write(path, &fs_path, text, expected.bom)
    }

    /// 比較せずに保存する。ファイルが無い場合は作る（Spec §6.2）。
    pub fn save_overwrite(
        &self,
        path: &VaultPath,
        text: &str,
        bom: bool,
    ) -> Result<FileState, VaultError> {
        let fs_path = match self.check_regular_file(path) {
            Ok(p) => p,
            Err(VaultError::NotFound { .. }) => path.to_fs_path(&self.root),
            Err(e) => return Err(e),
        };
        self.write(path, &fs_path, text, bom)
    }

    fn write(
        &self,
        path: &VaultPath,
        fs_path: &std::path::Path,
        text: &str,
        bom: bool,
    ) -> Result<FileState, VaultError> {
        let mut data = Vec::with_capacity(text.len() + BOM.len());
        if bom {
            data.extend_from_slice(BOM);
        }
        data.extend_from_slice(text.as_bytes());
        self.fs
            .write_atomic(fs_path, &data)
            .map_err(|e| io_err(path, e))?;
        Ok(FileState::of(&data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_matches_reference_values() {
        // FNV-1a 64 の公開されたテストベクター
        assert_eq!(fnv1a(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x85944171f73967e8);
    }
}
