//! Vault 内のパス（`docs/spec/vault.md` §2）。

use std::fmt;
use std::path::{Path, PathBuf};

/// Vault の根からの相対パス。`/` 区切りで、Vault 外を指せない。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultPath(String);

/// `VaultPath` の条件に反する理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidPathReason {
    Empty,
    EmptyComponent,
    DotComponent,
    ForbiddenChar,
}

impl fmt::Display for InvalidPathReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "empty path",
            Self::EmptyComponent => "empty path component (leading, trailing or repeated '/')",
            Self::DotComponent => "'.' or '..' component",
            Self::ForbiddenChar => "NUL or '\\' in path",
        })
    }
}

/// 1つの要素（ファイル名・ディレクトリ名）が条件を満たすか。
pub(crate) fn check_component(c: &str) -> Result<(), InvalidPathReason> {
    if c.is_empty() {
        Err(InvalidPathReason::EmptyComponent)
    } else if c == "." || c == ".." {
        Err(InvalidPathReason::DotComponent)
    } else if c.contains(['\0', '\\', '/']) {
        Err(InvalidPathReason::ForbiddenChar)
    } else {
        Ok(())
    }
}

impl VaultPath {
    /// `/` 区切りの文字列から作る。正規化・変換をしない。
    pub fn new(s: &str) -> Result<Self, InvalidPathReason> {
        if s.is_empty() {
            return Err(InvalidPathReason::Empty);
        }
        for c in s.split('/') {
            check_component(c)?;
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 要素の列。
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// 最後の要素（ファイル名）。
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// 子の要素を加えたパス。
    pub(crate) fn join(parent: Option<&VaultPath>, name: &str) -> Self {
        match parent {
            Some(p) => Self(format!("{}/{name}", p.0)),
            None => Self(name.to_string()),
        }
    }

    /// Vault の根の絶対パスに連結する。
    pub fn to_fs_path(&self, root: &Path) -> PathBuf {
        let mut p = root.to_path_buf();
        p.extend(self.components());
        p
    }
}

impl fmt::Display for VaultPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn accepts_relative_paths_as_is() {
        for s in [
            "note.md",
            "フォルダー/ノート 1.md",
            "a/b/c",
            ".obsidian/app.json",
            "e\u{301}.md",
        ] {
            assert_eq!(VaultPath::new(s).unwrap().as_str(), s);
        }
        assert_eq!(VaultPath::new("a/b/c.md").unwrap().file_name(), "c.md");
    }

    #[test]
    fn rejects_paths_that_could_escape_or_are_ambiguous() {
        use InvalidPathReason::*;
        let cases = [
            ("", Empty),
            ("/abs.md", EmptyComponent),
            ("a/", EmptyComponent),
            ("a//b", EmptyComponent),
            ("..", DotComponent),
            ("a/../b", DotComponent),
            ("./a", DotComponent),
            ("a\\b", ForbiddenChar),
            ("a\0b", ForbiddenChar),
        ];
        for (s, reason) in cases {
            assert_eq!(VaultPath::new(s), Err(reason), "{s:?}");
        }
    }

    proptest! {
        /// 受理したパスを根に連結した結果は常に根の内側にある（Spec §8）。
        #[test]
        fn accepted_paths_stay_inside_root(parts in prop::collection::vec("[a-z.\\\\/\0 あ]{0,4}", 1..5)) {
            let s = parts.join("/");
            if let Ok(p) = VaultPath::new(&s) {
                let root = Path::new("/vault/root");
                let full = p.to_fs_path(root);
                prop_assert!(full.starts_with(root));
                prop_assert!(full.components().count() > root.components().count());
                prop_assert!(full.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::RootDir)));
            }
        }
    }
}
