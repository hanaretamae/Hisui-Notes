//! 検証用のMCPサーバー（stdio）。
use protocol_clients::mcp_server::WordCounter;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = WordCounter::default().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
