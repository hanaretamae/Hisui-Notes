//! 開発用の検査。`cargo xtask deps` で ADR-0002 の依存の規則を検査する。
//!
//! - ワークスペース内のクレートの直接の依存（通常の依存）が、ADR-0002 の表の範囲内か。
//! - `ui-gpui`・`hisui-notes` 以外のクレートが、GPUI に（推移的にも）依存していないか。
//!
//! 開発時の依存（dev-dependencies）は対象外とする（テストでの組み立てを許すため）。

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::process::{Command, ExitCode};

use serde_json::Value;

/// ADR-0002 の「依存してよい先」（ワークスペース内のクレート）。`None` は「すべて」。
fn allowed(name: &str) -> Option<Option<&'static [&'static str]>> {
    Some(match name {
        "app-api" => Some(&[]),
        "text-buffer" => Some(&[]),
        "document" => Some(&["text-buffer", "app-api"]),
        "vault" => Some(&["document", "platform", "app-api"]),
        "parser-markdown" => Some(&["document"]),
        "platform" => Some(&["app-api"]),
        "app-core" => Some(&[
            "app-api",
            "text-buffer",
            "document",
            "vault",
            "parser-markdown",
            "platform",
        ]),
        "ui-gpui" => Some(&["app-api", "app-core"]),
        "hisui-notes" => None,
        "xtask" => Some(&[]),
        _ => return None,
    })
}

/// GPUI への依存を許すクレート（ADR-0002: app-core 以下は GPUI に依存しない）。
const GPUI_ALLOWED: &[&str] = &["ui-gpui", "hisui-notes"];
const GPUI_CRATES: &[&str] = &[
    "gpui",
    "gpui_platform",
    "gpui_wgpu",
    "gpui_linux",
    "gpui_macos",
    "gpui_windows",
    "gpui_ios",
];

fn metadata() -> Value {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .expect("run cargo metadata");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("parse cargo metadata")
}

fn check_deps() -> Vec<String> {
    let m = metadata();
    let members: BTreeSet<String> = m["workspace_members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let name_of: HashMap<String, String> = m["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["id"].as_str().unwrap().to_string(),
                p["name"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    // 通常・ビルド時の依存の辺
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for node in m["resolve"]["nodes"].as_array().unwrap() {
        let id = node["id"].as_str().unwrap().to_string();
        let deps = node["deps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| {
                d["dep_kinds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|k| k["kind"].as_str() != Some("dev"))
            })
            .map(|d| d["pkg"].as_str().unwrap().to_string())
            .collect();
        edges.insert(id, deps);
    }
    let mut errors = vec![];
    let mut summary: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for id in &members {
        let name = &name_of[id];
        let Some(rule) = allowed(name) else {
            errors.push(format!(
                "{name}: ADR-0002 の表に無いクレート（表とこの検査を更新する）"
            ));
            continue;
        };
        let direct: Vec<String> = edges[id]
            .iter()
            .filter(|d| members.contains(*d))
            .map(|d| name_of[d].clone())
            .collect();
        if let Some(ok) = rule {
            for d in &direct {
                if !ok.contains(&d.as_str()) {
                    errors.push(format!("{name} -> {d}: ADR-0002 で許されていない依存"));
                }
            }
        }
        summary.insert(name.clone(), direct);
        if !GPUI_ALLOWED.contains(&name.as_str()) {
            // 推移的に GPUI に到達するか
            let mut stack = vec![id.clone()];
            let mut seen = BTreeSet::new();
            while let Some(n) = stack.pop() {
                if !seen.insert(n.clone()) {
                    continue;
                }
                if GPUI_CRATES.contains(&name_of[&n].as_str()) {
                    errors.push(format!("{name}: GPUI（{}）に依存している", name_of[&n]));
                    break;
                }
                stack.extend(edges[&n].iter().cloned());
            }
        }
    }
    for (n, d) in &summary {
        println!(
            "{n:<16} -> {}",
            if d.is_empty() {
                "(none)".to_string()
            } else {
                d.join(", ")
            }
        );
    }
    errors
}

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("deps") => {
            let errors = check_deps();
            if errors.is_empty() {
                println!("deps: ok");
                ExitCode::SUCCESS
            } else {
                for e in errors {
                    eprintln!("error: {e}");
                }
                ExitCode::FAILURE
            }
        }
        _ => {
            eprintln!("usage: cargo xtask deps");
            ExitCode::FAILURE
        }
    }
}
