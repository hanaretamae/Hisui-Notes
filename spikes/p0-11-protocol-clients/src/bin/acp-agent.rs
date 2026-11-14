//! 検証用のACPエージェント（stdio）。プロンプトの文字列を1回のメッセージとして返す。
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::v1::schema::{
    AgentCapabilities, ContentBlock, ContentChunk, Implementation, InitializeRequest, InitializeResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, SessionNotification, SessionUpdate, StopReason, TextContent,
};
use agent_client_protocol::{Agent, Client, Responder, Stdio, V1};

#[tokio::main]
async fn main() -> agent_client_protocol::Result<()> {
    Agent
        .v1()
        .name("hisui-p0-11-agent")
        .on_receive_request(
            async move |init: InitializeRequest, responder: Responder<InitializeResponse, V1>, _c: ConnectionTo<Client>| {
                responder.respond(
                    InitializeResponse::new(init.protocol_version)
                        .agent_capabilities(AgentCapabilities::new())
                        .agent_info(Implementation::new("hisui-p0-11-agent", "0.0.0")),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_r: NewSessionRequest, responder: Responder<NewSessionResponse, V1>, _c: ConnectionTo<Client>| {
                responder.respond(NewSessionResponse::new("session-1"))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |r: PromptRequest, responder: Responder<PromptResponse, V1>, c: ConnectionTo<Client>| {
                let text: Vec<String> = r
                    .prompt
                    .iter()
                    .filter_map(|b| if let ContentBlock::Text(t) = b { Some(t.text.clone()) } else { None })
                    .collect();
                c.send_notification(SessionNotification::new(
                    r.session_id,
                    SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(TextContent::new(format!(
                        "echo: {}",
                        text.join(" ")
                    ))))),
                ))?;
                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(Stdio::new())
        .await
}
