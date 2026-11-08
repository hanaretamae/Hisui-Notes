//! P0-11: LSP・MCP・ACPの最小クライアントの検証。製品コードではない。

pub mod mcp_server {
    use rmcp::{
        ServerHandler,
        handler::server::{router::tool::ToolRouter, wrapper::Parameters},
        model::{ServerCapabilities, ServerConfig},
        schemars, tool, tool_handler, tool_router,
    };

    #[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
    pub struct WordCountRequest {
        /// 数える文字列
        pub text: String,
    }

    /// 検証用のMCPサーバー（ツール1つ）。
    #[derive(Debug, Clone)]
    pub struct WordCounter {
        tool_router: ToolRouter<Self>,
    }

    impl Default for WordCounter {
        fn default() -> Self {
            Self { tool_router: Self::tool_router() }
        }
    }

    #[tool_router]
    impl WordCounter {
        #[tool(description = "Count whitespace-separated words and characters")]
        fn word_count(&self, Parameters(WordCountRequest { text }): Parameters<WordCountRequest>) -> String {
            format!("words={} chars={}", text.split_whitespace().count(), text.chars().count())
        }
    }

    #[tool_handler(router = self.tool_router)]
    impl ServerHandler for WordCounter {
        fn get_info(&self) -> ServerConfig {
            ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
        }
    }
}
