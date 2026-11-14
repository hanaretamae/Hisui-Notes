//! ADR-0008 の Permission Broker の最小実装（検証用）。

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    Read,
    External,
}

pub fn risk(cap: &str) -> Option<Risk> {
    match cap {
        "vault.read" => Some(Risk::Read),
        "net.connect" => Some(Risk::External),
        _ => None,
    }
}

/// Vault相対パスを正規化する。Vault外を指す可能性のあるパスは `None`（拒否）。
pub fn normalize_vault_path(p: &str) -> Option<Vec<&str>> {
    if p.is_empty() || p.starts_with('/') || p.contains('\\') || p.contains(':') || p.contains('\0') {
        return None;
    }
    let mut out = vec![];
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => return None,
            c => out.push(c),
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

pub struct Broker {
    generation: AtomicU64,
    /// (主体, 権限) → 範囲の一覧（vault.read: 正規化済みのパス接頭辞、net.connect: オリジン）
    grants: RwLock<HashMap<(String, String), Vec<String>>>,
}

impl Broker {
    pub fn new() -> Self {
        Broker { generation: AtomicU64::new(1), grants: RwLock::new(HashMap::new()) }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn grant(&self, principal: &str, cap: &str, scope: &str) {
        self.grants.write().unwrap().entry((principal.into(), cap.into())).or_default().push(scope.into());
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn revoke(&self, principal: &str, cap: &str) {
        self.grants.write().unwrap().remove(&(principal.to_string(), cap.to_string()));
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// 判定できない場合は Deny（ADR-0008 §5）。
    pub fn check(&self, principal: &str, cap: &str, scope: &str) -> Decision {
        let Some(risk) = risk(cap) else { return Decision::Deny };
        let grants = self.grants.read().unwrap();
        let Some(scopes) = grants.get(&(principal.to_string(), cap.to_string())) else { return Decision::Deny };
        let matched = match cap {
            "vault.read" => {
                let Some(path) = normalize_vault_path(scope) else { return Decision::Deny };
                scopes.iter().any(|g| {
                    let g: Vec<&str> = g.split('/').filter(|c| !c.is_empty()).collect();
                    path.len() >= g.len() && path[..g.len()] == g[..]
                })
            }
            "net.connect" => scopes.iter().any(|g| g == scope),
            _ => false,
        };
        match (matched, risk) {
            (false, _) => Decision::Deny,
            (true, Risk::Read) => Decision::Allow,
            // external は付与があっても操作ごとに確認する（ADR-0008 §4）
            (true, Risk::External) => Decision::Confirm,
        }
    }
}
