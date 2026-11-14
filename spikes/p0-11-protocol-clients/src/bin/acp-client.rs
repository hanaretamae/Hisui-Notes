//! ACPの最小クライアント: エージェントを子プロセス（stdio）で起動し、
//! 初期化・Capabilityの交換・セッション作成・プロンプト1往復を行う。
//! 権限の要求は既定で拒否する（ADR-0008。検証用のSDKの例は自動で許可するが、それに倣わない）。
use std::str::FromStr;
use std::time::Instant;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::v1::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SessionNotification, TextContent,
};
use agent_client_protocol::{AcpAgent, Agent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent_path = std::env::current_exe()?.with_file_name("acp-agent");
    let config = serde_json::json!({ "command": agent_path, "args": [] }).to_string();
    let agent = AcpAgent::from_str(&config)?;
    let t0 = Instant::now();
    agent_client_protocol::Client
        .v1()
        .on_receive_notification(
            async move |n: SessionNotification, _cx| {
                println!("[acp] session/update: {:?}", n.update);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |r: RequestPermissionRequest, responder, _c| {
                println!("[acp] permission requested, denied by default: {:?}", r.tool_call);
                responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
            let init = connection.send_request(InitializeRequest::new(ProtocolVersion::V1)).block_task().await?;
            println!("[acp] initialized in {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
            println!("[acp] protocol version: {:?}", init.protocol_version);
            println!("[acp] agent: {:?}", init.agent_info);
            println!("[acp] agent capabilities: {:?}", init.agent_capabilities);
            let s = connection.send_request(NewSessionRequest::new(std::env::temp_dir())).block_task().await?;
            println!("[acp] session: {:?}", s.session_id);
            let t = Instant::now();
            let p = connection
                .send_request(PromptRequest::new(s.session_id, vec![ContentBlock::Text(TextContent::new("こんにちは Hisui"))]))
                .block_task()
                .await?;
            println!("[acp] prompt -> stop_reason {:?} in {:.2} ms", p.stop_reason, t.elapsed().as_secs_f64() * 1e3);
            Ok(())
        })
        .await?;
    Ok(())
}
