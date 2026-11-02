//! Optional stdio adapter. Broker access is independent of server startup.
mod error;
#[cfg(test)]
mod error_tests;
mod pagination;
mod schema;
mod tools;
#[cfg(test)]
mod version_tests;

use crate::broker_client::{Credentials, ReadClient};
use clap::Parser;
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerConfig,
    },
    service::RequestContext,
    transport::stdio,
};

#[derive(Parser)]
#[command(name = "pactling mcp", disable_version_flag = true)]
struct Args {
    #[arg(long, env = "PACT_BROKER_BASE_URL", hide_env_values = true)]
    broker_base_url: String,
    #[arg(
        long,
        short = 'u',
        env = "PACT_BROKER_USERNAME",
        hide_env_values = true
    )]
    broker_username: Option<String>,
    #[arg(
        long,
        short = 'p',
        env = "PACT_BROKER_PASSWORD",
        hide_env_values = true
    )]
    broker_password: Option<String>,
    #[arg(long, short = 'k', env = "PACT_BROKER_TOKEN", hide_env_values = true)]
    broker_token: Option<String>,
}

/// Serve only MCP frames on stdout. No tracing subscriber, database or listener is started.
pub async fn run() -> anyhow::Result<()> {
    let args = match Args::try_parse_from(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(error) if !error.use_stderr() => {
            eprint!("{error}");
            return Ok(());
        }
        Err(_) => anyhow::bail!("Invalid MCP options; use pactling mcp --help"),
    };
    let client = ReadClient::new(
        &args.broker_base_url,
        Credentials {
            username: args.broker_username,
            password: args.broker_password,
            token: args.broker_token,
        },
    )?;
    McpServer { client }.serve(stdio()).await?.waiting().await?;
    Ok(())
}

#[derive(Clone)]
pub struct McpServer {
    client: ReadClient,
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        tools::call(&self.client, request).await.map(Into::into)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(tools::definitions()))
    }
}
