//! MCPの最小クライアント: stdio（子プロセス）と Streamable HTTP（ループバック）で、
//! 初期化・Capabilityの交換・ツール一覧・ツール呼び出しを1往復ずつ行う。
use std::time::Instant;

use protocol_clients::mcp_server::WordCounter;
use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager};
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt, service::RunningService};
use tokio_util::sync::CancellationToken;

async fn exercise(name: &str, client: RunningService<RoleClient, ClientConfig>, t0: Instant) -> anyhow::Result<()> {
    let init = t0.elapsed();
    let info = client.peer_info().ok_or_else(|| anyhow::anyhow!("no peer info"))?;
    println!("[{name}] initialized in {:.1} ms", init.as_secs_f64() * 1e3);
    println!("[{name}] protocol version: {:?}", info.protocol_version);
    println!("[{name}] server: {:?}", info.server_info.as_ref().map(|s| (s.name.clone(), s.version.clone())));
    println!("[{name}] server capabilities: tools={} resources={} prompts={}", info.capabilities.tools.is_some(), info.capabilities.resources.is_some(), info.capabilities.prompts.is_some());
    let tools = client.list_all_tools().await?;
    println!("[{name}] tools: {:?}", tools.iter().map(|t| t.name.to_string()).collect::<Vec<_>>());
    let args = serde_json::from_value(serde_json::json!({ "text": "Hisui Notes は 文書 エディター です" }))?;
    let t = Instant::now();
    let r = client.call_tool(CallToolRequestParams::new("word_count").with_arguments(args)).await?;
    println!("[{name}] call_tool -> {:?} (is_error={:?}) in {:.2} ms", r.content.first().and_then(|c| c.as_text()).map(|t| t.text.clone()), r.is_error, t.elapsed().as_secs_f64() * 1e3);
    client.cancel().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdio: 同じディレクトリの mcp-server を子プロセスとして起動する
    let server = std::env::current_exe()?.with_file_name("mcp-server");
    let t0 = Instant::now();
    let client = ClientConfig::default().serve(TokioChildProcess::new(tokio::process::Command::new(server))?).await?;
    exercise("stdio", client, t0).await?;

    // Streamable HTTP: ループバックにサーバーを立てる
    let ct = CancellationToken::new();
    let service: StreamableHttpService<WordCounter, LocalSessionManager> = StreamableHttpService::new(
        || Ok(WordCounter::default()),
        Default::default(),
        StreamableHttpServerConfig::default().with_sse_keep_alive(None).with_cancellation_token(ct.child_token()),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn({
        let ct = ct.clone();
        async move {
            let _ = axum::serve(listener, router).with_graceful_shutdown(async move { ct.cancelled_owned().await }).await;
        }
    });
    let t0 = Instant::now();
    let transport = StreamableHttpClientTransport::from_config(StreamableHttpClientTransportConfig::with_uri(format!("http://{addr}/mcp")));
    let client = ClientConfig::default().serve(transport).await?;
    exercise("streamable-http", client, t0).await?;
    ct.cancel();
    handle.await?;
    Ok(())
}
