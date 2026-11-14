//! `docs/spec/vault.md` の規則の検証。メモリ上のファイルシステムと、デスクトップの実ファイルで確かめる。

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use platform::{DesktopFileSystem, DirEntry, EntryKind, FileSystem};
use vault::{ConflictKind, FileState, InvalidPathReason, SkipReason, Vault, VaultError, VaultPath};

#[derive(Debug, Clone)]
enum Node {
    File(Vec<u8>),
    Dir,
    Symlink(PathBuf),
}

/// メモリ上のファイルシステム。名前に任意のバイト列（UTF-8 でない名前）を使える。
#[derive(Default)]
struct MemFs {
    nodes: Mutex<BTreeMap<PathBuf, Node>>,
    writes: Mutex<Vec<PathBuf>>,
}

impl MemFs {
    fn with(entries: &[(&str, Node)]) -> Arc<Self> {
        let fs = Self::default();
        {
            let mut n = fs.nodes.lock().unwrap();
            n.insert(PathBuf::from("/v"), Node::Dir);
            for (p, node) in entries {
                n.insert(PathBuf::from("/v").join(p), node.clone());
            }
        }
        Arc::new(fs)
    }

    fn insert(&self, path: impl Into<PathBuf>, node: Node) {
        self.nodes.lock().unwrap().insert(path.into(), node);
    }

    fn file(&self, path: &str) -> Option<Vec<u8>> {
        match self.nodes.lock().unwrap().get(Path::new(path)) {
            Some(Node::File(b)) => Some(b.clone()),
            _ => None,
        }
    }

    fn resolve(&self, path: &Path) -> io::Result<Node> {
        let nodes = self.nodes.lock().unwrap();
        let mut node = nodes.get(path).cloned().ok_or(io::ErrorKind::NotFound)?;
        for _ in 0..8 {
            match node {
                Node::Symlink(t) => node = nodes.get(&t).cloned().ok_or(io::ErrorKind::NotFound)?,
                n => return Ok(n),
            }
        }
        Err(io::Error::other("symlink loop"))
    }
}

fn kind_of(n: &Node) -> EntryKind {
    match n {
        Node::File(_) => EntryKind::File,
        Node::Dir => EntryKind::Dir,
        Node::Symlink(_) => EntryKind::Symlink,
    }
}

impl FileSystem for MemFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        match self.resolve(path)? {
            Node::File(b) => Ok(b),
            _ => Err(io::ErrorKind::IsADirectory.into()),
        }
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        if !matches!(self.resolve(path)?, Node::Dir) {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let nodes = self.nodes.lock().unwrap();
        Ok(nodes
            .iter()
            .filter(|(p, _)| p.parent() == Some(path))
            .map(|(p, n)| DirEntry {
                name: p.file_name().unwrap().to_owned(),
                kind: kind_of(n),
            })
            .collect())
    }

    fn kind(&self, path: &Path, follow: bool) -> io::Result<EntryKind> {
        if follow {
            return Ok(kind_of(&self.resolve(path)?));
        }
        self.nodes
            .lock()
            .unwrap()
            .get(path)
            .map(kind_of)
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn write_atomic(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        if !matches!(self.resolve(path.parent().unwrap())?, Node::Dir) {
            return Err(io::ErrorKind::NotFound.into());
        }
        self.writes.lock().unwrap().push(path.to_path_buf());
        self.insert(path, Node::File(data.to_vec()));
        Ok(())
    }
}

fn file(s: &str) -> Node {
    Node::File(s.as_bytes().to_vec())
}

fn p(s: &str) -> VaultPath {
    VaultPath::new(s).unwrap()
}

fn open(fs: &Arc<MemFs>) -> Vault {
    Vault::open("/v", fs.clone()).unwrap()
}

// --- §4 開く・一覧 ---

#[test]
fn open_requires_directory() {
    let fs = MemFs::with(&[("note.md", file("x"))]);
    assert!(matches!(
        Vault::open("/v/note.md", fs.clone()),
        Err(VaultError::InvalidRoot { .. })
    ));
    assert!(matches!(
        Vault::open("/nope", fs.clone()),
        Err(VaultError::InvalidRoot {
            kind: io::ErrorKind::NotFound
        })
    ));
    fs.insert("/link", Node::Symlink("/v".into()));
    assert!(
        Vault::open("/link", fs).is_ok(),
        "根はシンボリックリンクの先がディレクトリでもよい"
    );
}

#[test]
fn list_excludes_hidden_and_symlinks_and_sorts() {
    let fs = MemFs::with(&[
        ("b.md", file("")),
        ("a", Node::Dir),
        ("a/z.md", file("")),
        ("a/画像.png", file("")),
        (".obsidian", Node::Dir),
        (".obsidian/app.json", file("{}")),
        ("a/.hidden.md", file("")),
        ("link.md", Node::Symlink("/v/b.md".into())),
        ("linkdir", Node::Symlink("/v/a".into())),
        ("back\\slash.md", file("")),
    ]);
    fs.insert(PathBuf::from("/v").join(non_utf8_name()), file(""));
    let listing = open(&fs).list().unwrap();
    assert_eq!(listing.files, vec![p("a/z.md"), p("a/画像.png"), p("b.md")]);
    let mut reasons: Vec<SkipReason> = listing.skipped.iter().map(|s| s.reason).collect();
    reasons.sort_by_key(|r| format!("{r:?}"));
    assert_eq!(
        reasons,
        vec![
            SkipReason::InvalidName(InvalidPathReason::ForbiddenChar),
            SkipReason::NonUtf8Name
        ]
    );
}

#[cfg(unix)]
fn non_utf8_name() -> OsString {
    use std::os::unix::ffi::OsStringExt as _;
    OsString::from_vec(vec![b'n', 0xff, b'.', b'm', b'd'])
}

#[cfg(not(unix))]
fn non_utf8_name() -> OsString {
    OsString::from("placeholder\\name")
}

// --- §5 読み込み ---

#[test]
fn load_returns_text_without_bom_and_keeps_bytes() {
    let fs = MemFs::with(&[
        (
            "bom.md",
            Node::File(b"\xEF\xBB\xBF# \xE8\xA6\x8B\r\n".to_vec()),
        ),
        ("plain.md", file("a\u{feff}b")),
    ]);
    let v = open(&fs);
    let l = v.load(&p("bom.md")).unwrap();
    assert_eq!(l.text, "# 見\r\n");
    assert!(l.state.bom);
    assert_eq!(l.state.len, 10);
    let l = v.load(&p("plain.md")).unwrap();
    assert_eq!(l.text, "a\u{feff}b", "先頭以外の U+FEFF は内容");
    assert!(!l.state.bom);
}

#[test]
fn load_rejects_invalid_utf8_and_non_files() {
    let fs = MemFs::with(&[
        ("sjis.md", Node::File(b"\xEF\xBB\xBFok\x82\xA0".to_vec())),
        ("dir", Node::Dir),
        (".hidden.md", file("h")),
    ]);
    let v = open(&fs);
    assert_eq!(
        v.load(&p("sjis.md")),
        Err(VaultError::NotUtf8 {
            path: p("sjis.md"),
            valid_up_to: 2
        })
    );
    assert_eq!(
        v.load(&p("dir")),
        Err(VaultError::NotAFile { path: p("dir") })
    );
    assert_eq!(
        v.load(&p("none.md")),
        Err(VaultError::NotFound { path: p("none.md") })
    );
    assert_eq!(
        v.load(&p("dir/none.md")),
        Err(VaultError::NotFound {
            path: p("dir/none.md")
        })
    );
    assert_eq!(
        v.load(&p(".hidden.md")).unwrap().text,
        "h",
        "隠しファイルもパスを指定すれば開ける"
    );
}

#[test]
fn load_refuses_symlinks_anywhere_in_path() {
    let fs = MemFs::with(&[("real", Node::Dir), ("real/n.md", file("x"))]);
    fs.insert("/v/l.md", Node::Symlink("/v/real/n.md".into()));
    fs.insert("/v/ldir", Node::Symlink("/v/real".into()));
    fs.insert("/v/out.md", Node::Symlink("/etc/passwd".into()));
    let v = open(&fs);
    for path in ["l.md", "ldir/n.md", "out.md"] {
        assert_eq!(
            v.load(&p(path)),
            Err(VaultError::Symlink { path: p(path) }),
            "{path}"
        );
    }
}

// --- §6 保存 ---

#[test]
fn save_restores_bom_and_returns_new_state() {
    let fs = MemFs::with(&[("n.md", Node::File(b"\xEF\xBB\xBFold".to_vec()))]);
    let v = open(&fs);
    let l = v.load(&p("n.md")).unwrap();
    let st = v.save(&p("n.md"), "新", &l.state).unwrap();
    assert_eq!(fs.file("/v/n.md").unwrap(), b"\xEF\xBB\xBF\xE6\x96\xB0");
    assert!(st.bom);
    // 続けて保存できる（前回の保存の状態を使う）
    v.save(&p("n.md"), "2", &st).unwrap();
    assert_eq!(fs.file("/v/n.md").unwrap(), b"\xEF\xBB\xBF2");
}

#[test]
fn save_detects_external_changes() {
    let fs = MemFs::with(&[("n.md", file("v1"))]);
    let v = open(&fs);
    let l = v.load(&p("n.md")).unwrap();
    fs.insert("/v/n.md", file("v2"));
    assert_eq!(
        v.save(&p("n.md"), "mine", &l.state),
        Err(VaultError::Conflict {
            path: p("n.md"),
            kind: ConflictKind::Modified
        })
    );
    assert_eq!(fs.file("/v/n.md").unwrap(), b"v2", "衝突時は書き込まない");
    // 同じ長さの変更も検出する
    fs.insert("/v/n.md", file("v1"));
    let l = v.load(&p("n.md")).unwrap();
    fs.insert("/v/n.md", file("V1"));
    assert!(matches!(
        v.save(&p("n.md"), "mine", &l.state),
        Err(VaultError::Conflict {
            kind: ConflictKind::Modified,
            ..
        })
    ));
    // 削除
    fs.nodes.lock().unwrap().remove(Path::new("/v/n.md"));
    assert_eq!(
        v.save(&p("n.md"), "mine", &l.state),
        Err(VaultError::Conflict {
            path: p("n.md"),
            kind: ConflictKind::Deleted
        })
    );
    assert!(fs.writes.lock().unwrap().is_empty());
    // 上書きを選んだ場合
    let st = v.save_overwrite(&p("n.md"), "mine", false).unwrap();
    assert_eq!(fs.file("/v/n.md").unwrap(), b"mine");
    assert_eq!(
        st,
        FileState {
            bom: false,
            len: 4,
            hash: st.hash
        }
    );
}

#[test]
fn save_refuses_symlinks() {
    let fs = MemFs::with(&[("real.md", file("x"))]);
    fs.insert("/v/l.md", Node::Symlink("/v/real.md".into()));
    let v = open(&fs);
    let st = v.load(&p("real.md")).unwrap().state;
    assert_eq!(
        v.save(&p("l.md"), "y", &st),
        Err(VaultError::Symlink { path: p("l.md") })
    );
    assert_eq!(
        v.save_overwrite(&p("l.md"), "y", false),
        Err(VaultError::Symlink { path: p("l.md") })
    );
    assert!(fs.writes.lock().unwrap().is_empty());
}

// --- デスクトップの実ファイルでの通し確認 ---

#[test]
fn desktop_round_trip() {
    let root = std::env::temp_dir().join(format!("hisui-vault-test-{}", std::process::id()));
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/n.md"), b"\xEF\xBB\xBFline\r\n").unwrap();
    let result = std::panic::catch_unwind(|| {
        let v = Vault::open(&root, Arc::new(DesktopFileSystem)).unwrap();
        assert_eq!(v.list().unwrap().files, vec![p("sub/n.md")]);
        let l = v.load(&p("sub/n.md")).unwrap();
        assert_eq!(l.text, "line\r\n");
        v.save(&p("sub/n.md"), "line\r\n編集\r\n", &l.state)
            .unwrap();
        assert_eq!(
            std::fs::read(root.join("sub/n.md")).unwrap(),
            "\u{feff}line\r\n編集\r\n".as_bytes()
        );
        assert_eq!(
            v.list().unwrap().files,
            vec![p("sub/n.md")],
            "一時ファイルが残らない"
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("sub/n.md"), root.join("l.md")).unwrap();
            assert_eq!(
                v.load(&p("l.md")),
                Err(VaultError::Symlink { path: p("l.md") })
            );
            assert_eq!(v.list().unwrap().files, vec![p("sub/n.md")]);
        }
    });
    let _ = std::fs::remove_dir_all(&root);
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
