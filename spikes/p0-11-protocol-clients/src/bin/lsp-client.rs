//! LSPの最小クライアント: 言語サーバーを子プロセス（stdio）で起動し、
//! initialize・Capabilityの確認・文書を開く・documentSymbol を1往復行う。
//! JSON-RPC のフレーミング（Content-Length）は自前で実装し、結果の型に lsp-types を使う。
//!
//! 使い方: lsp-client <言語サーバーの実行ファイル> [引数...]（例: marksman server）

use std::time::Instant;

use anyhow::{Context, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};

struct Conn {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Conn {
    async fn send(&mut self, msg: Value) -> anyhow::Result<()> {
        let body = serde_json::to_vec(&msg)?;
        self.stdin.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes()).await?;
        self.stdin.write_all(&body).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn recv(&mut self) -> anyhow::Result<Value> {
        let mut len = None;
        loop {
            let mut line = String::new();
            if self.stdout.read_line(&mut line).await? == 0 {
                return Err(anyhow!("server closed the connection"));
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(v) = line.strip_prefix("Content-Length: ") {
                len = Some(v.parse::<usize>()?);
            }
        }
        let mut buf = vec![0; len.context("missing Content-Length")?];
        self.stdout.read_exact(&mut buf).await?;
        Ok(serde_json::from_slice(&buf)?)
    }

    async fn notify(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params })).await
    }

    /// 要求を送り、応答を待つ。その間にサーバーから届いた要求には既定の応答を返し、通知は記録する。
    async fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await?;
        loop {
            let m = self.recv().await?;
            match (m.get("id"), m.get("method")) {
                (Some(rid), None) if rid == &json!(id) => {
                    if let Some(e) = m.get("error") {
                        return Err(anyhow!("{method} failed: {e}"));
                    }
                    return Ok(m["result"].clone());
                }
                (Some(rid), Some(sm)) => {
                    // サーバーからの要求（workDoneProgress/create、client/registerCapability 等）
                    println!("[lsp] server request {sm} -> null");
                    self.send(json!({ "jsonrpc": "2.0", "id": rid, "result": null })).await?;
                }
                (None, Some(sm)) => println!("[lsp] server notification {sm}"),
                _ => println!("[lsp] unexpected message {m}"),
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, cmd_args) = args.split_first().context("usage: lsp-client <server> [args...]")?;

    // 検証用のワークスペース（一時ディレクトリ）
    let dir = std::env::temp_dir().join(format!("hisui-p0-11-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let doc_path = dir.join("note.md");
    let text = "# 見出し\n\n本文 [[other]] と [リンク](other.md#節)。\n\n## 小見出し 🇯🇵\n\n- 項目\n";
    std::fs::write(&doc_path, text)?;
    std::fs::write(dir.join("other.md"), "# other\n\n## 節\n")?;
    let root_uri = format!("file://{}", dir.display());
    let doc_uri = format!("file://{}", doc_path.display());

    let mut child = Command::new(cmd)
        .args(cmd_args)
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut c = Conn { stdin: child.stdin.take().unwrap(), stdout: BufReader::new(child.stdout.take().unwrap()), next_id: 0 };

    let t0 = Instant::now();
    let init = c
        .request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": { "name": "hisui-p0-11", "version": "0.0.0" },
                "rootUri": root_uri,
                "workspaceFolders": [{ "uri": root_uri, "name": "p0-11" }],
                "capabilities": {
                    "general": { "positionEncodings": ["utf-8", "utf-16"] },
                    "textDocument": { "documentSymbol": { "hierarchicalDocumentSymbolSupport": true } },
                    "workspace": { "workspaceFolders": true }
                }
            }),
        )
        .await?;
    let init: lsp_types::InitializeResult = serde_json::from_value(init)?;
    println!("[lsp] initialized in {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
    println!("[lsp] server: {:?}", init.server_info.as_ref().map(|s| (&s.name, &s.version)));
    let caps = &init.capabilities;
    println!(
        "[lsp] capabilities: positionEncoding={:?} documentSymbol={} definition={} completion={} hover={} rename={}",
        caps.position_encoding,
        caps.document_symbol_provider.is_some(),
        caps.definition_provider.is_some(),
        caps.completion_provider.is_some(),
        caps.hover_provider.is_some(),
        caps.rename_provider.is_some()
    );
    c.notify("initialized", json!({})).await?;
    c.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": doc_uri, "languageId": "markdown", "version": 1, "text": text } }),
    )
    .await?;

    let t = Instant::now();
    let r = c.request("textDocument/documentSymbol", json!({ "textDocument": { "uri": doc_uri } })).await?;
    let symbols: lsp_types::DocumentSymbolResponse = serde_json::from_value(r)?;
    println!("[lsp] documentSymbol in {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
    fn walk(s: &[lsp_types::DocumentSymbol], depth: usize) {
        for x in s {
            println!("[lsp]   {}{} {:?} @{}:{}", "  ".repeat(depth), x.name, x.kind, x.range.start.line, x.range.start.character);
            if let Some(ch) = &x.children {
                walk(ch, depth + 1);
            }
        }
    }
    match &symbols {
        lsp_types::DocumentSymbolResponse::Nested(s) => walk(s, 0),
        lsp_types::DocumentSymbolResponse::Flat(s) => {
            for x in s {
                println!("[lsp]   {} {:?}", x.name, x.kind);
            }
        }
    }

    c.request("shutdown", Value::Null).await?;
    c.notify("exit", Value::Null).await?;
    let status = child.wait().await?;
    println!("[lsp] server exited: {status}");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
