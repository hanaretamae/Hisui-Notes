//! ファイルシステムの抽象と、デスクトップの実装（`docs/spec/vault.md` §3、§6.1）。

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// ディレクトリの要素・パスの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
    Other,
}

/// ディレクトリの要素。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: OsString,
    pub kind: EntryKind,
}

/// Vault が使うファイル操作（Spec §3）。パスは OS の絶対パス。
pub trait FileSystem: Send + Sync {
    /// ファイルの内容。
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// ディレクトリの要素。順序は不定。要素の種類はシンボリックリンクを辿らない。
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>>;
    /// パスの種類。`follow` が偽ならシンボリックリンク自体の種類を返す。
    fn kind(&self, path: &Path, follow: bool) -> io::Result<EntryKind>;
    /// 一時ファイルへの書き込みと置き換えで、異常終了で元の内容を失わないように書き込む（Spec §6.1）。
    fn write_atomic(&self, path: &Path, data: &[u8]) -> io::Result<()>;
}

/// デスクトップ（std::fs）の実装。
#[derive(Debug, Default)]
pub struct DesktopFileSystem;

fn kind_of(ft: fs::FileType) -> EntryKind {
    if ft.is_symlink() {
        EntryKind::Symlink
    } else if ft.is_file() {
        EntryKind::File
    } else if ft.is_dir() {
        EntryKind::Dir
    } else {
        EntryKind::Other
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl FileSystem for DesktopFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        fs::read_dir(path)?
            .map(|e| {
                let e = e?;
                Ok(DirEntry {
                    name: e.file_name(),
                    kind: kind_of(e.file_type()?),
                })
            })
            .collect()
    }

    fn kind(&self, path: &Path, follow: bool) -> io::Result<EntryKind> {
        let meta = if follow {
            fs::metadata(path)?
        } else {
            fs::symlink_metadata(path)?
        };
        Ok(kind_of(meta.file_type()))
    }

    fn write_atomic(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let dir = path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        // 1. 同じディレクトリに `.` で始まる一時ファイルを新規に作る
        let mut tmp_name = OsString::from(".");
        tmp_name.push(name);
        tmp_name.push(format!(
            ".hisui-tmp-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let tmp = dir.join(tmp_name);
        let result = (|| {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            // 2. 書き込み、ディスクへの書き出しを待つ
            f.write_all(data)?;
            f.sync_all()?;
            // 3. 既存のファイルの権限を写す
            match fs::metadata(path) {
                Ok(meta) => f.set_permissions(meta.permissions())?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            drop(f);
            // 4. 置き換える
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
            return result;
        }
        // 5. ディレクトリの書き出しを待つ（対応しない OS ではディレクトリを開けないため無視する）
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// テスト用の一時ディレクトリ（終了時に削除する）。
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let p = std::env::temp_dir().join(format!(
                "hisui-platform-test-{tag}-{}-{}",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn write_atomic_replaces_content_and_leaves_no_temp_files() {
        let t = TempDir::new("replace");
        let p = t.0.join("note.md");
        fs::write(&p, "old").unwrap();
        DesktopFileSystem
            .write_atomic(&p, "新しい内容\r\n".as_bytes())
            .unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "新しい内容\r\n");
        assert_eq!(names(&t.0), vec!["note.md"]);
        // 存在しないファイルは作る
        let q = t.0.join("new.md");
        DesktopFileSystem.write_atomic(&q, b"x").unwrap();
        assert_eq!(fs::read(&q).unwrap(), b"x");
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        let t = TempDir::new("perm");
        let p = t.0.join("note.md");
        fs::write(&p, "old").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        DesktopFileSystem.write_atomic(&p, b"new").unwrap();
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[cfg(unix)]
    #[test]
    fn failed_write_keeps_original() {
        use std::os::unix::fs::PermissionsExt as _;
        let t = TempDir::new("fail");
        let p = t.0.join("note.md");
        fs::write(&p, "original").unwrap();
        // ディレクトリを書き込み不可にして、一時ファイルを作れないようにする
        fs::set_permissions(&t.0, fs::Permissions::from_mode(0o500)).unwrap();
        let r = DesktopFileSystem.write_atomic(&p, b"new");
        fs::set_permissions(&t.0, fs::Permissions::from_mode(0o700)).unwrap();
        if r.is_ok() {
            // root 権限では書き込み不可のディレクトリにも書けるため、この検証はできない
            eprintln!("skipped: directory permissions are not enforced (running as root?)");
            return;
        }
        assert_eq!(fs::read_to_string(&p).unwrap(), "original");
        assert_eq!(names(&t.0), vec!["note.md"]);
    }

    #[cfg(unix)]
    #[test]
    fn kinds_distinguish_symlinks() {
        let t = TempDir::new("kind");
        let f = t.0.join("f");
        fs::write(&f, "").unwrap();
        let l = t.0.join("l");
        std::os::unix::fs::symlink(&f, &l).unwrap();
        assert_eq!(
            DesktopFileSystem.kind(&l, false).unwrap(),
            EntryKind::Symlink
        );
        assert_eq!(DesktopFileSystem.kind(&l, true).unwrap(), EntryKind::File);
        assert_eq!(DesktopFileSystem.kind(&t.0, false).unwrap(), EntryKind::Dir);
        let mut entries = DesktopFileSystem.read_dir(&t.0).unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(
            entries.iter().map(|e| e.kind).collect::<Vec<_>>(),
            vec![EntryKind::File, EntryKind::Symlink]
        );
    }
}
