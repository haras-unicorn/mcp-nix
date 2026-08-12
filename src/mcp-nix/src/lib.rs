//! mcp-nix - MCP server that provides nix tooling.

#![deny(unsafe_code)]
#![deny(
  clippy::unwrap_used,
  clippy::expect_used,
  clippy::panic,
  clippy::unreachable
)]
#![deny(clippy::arithmetic_side_effects)]
#![deny(clippy::todo)]
#![deny(clippy::allow_attributes_without_reason)]

mod nix;

use rmcp::ErrorData;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::schemars;
use rmcp::tool;
use rmcp::tool_router;

pub use nix::{NixError, build_package};

/// Parameters for the `nix_build` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BuildParams {
  /// A nix package reference, for example `nixpkgs#hello`.
  #[schemars(
    description = "A nix package reference, for example `nixpkgs#hello`."
  )]
  pub package: String,
}

/// MCP server that provides nix tooling.
#[derive(Debug, Clone)]
pub struct NixServer;

#[tool_router(server_handler)]
impl NixServer {
  /// Build a nix package and return its nix store path.
  #[tool(description = "Build a nix package and return its nix store path.")]
  async fn nix_build(
    &self,
    Parameters(params): Parameters<BuildParams>,
  ) -> Result<CallToolResult, ErrorData> {
    let package = params.package;

    match tokio::task::spawn_blocking(move || build_package(&package)).await {
      Ok(Ok(store_path)) => {
        Ok(CallToolResult::success(vec![ContentBlock::text(
          store_path,
        )]))
      }
      Ok(Err(error)) => Ok(CallToolResult::error(vec![ContentBlock::text(
        error.to_string(),
      )])),
      Err(join_error) => {
        Err(ErrorData::internal_error(join_error.to_string(), None))
      }
    }
  }
}
