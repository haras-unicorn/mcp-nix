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
mod sandbox;
mod sandbox_defaults;

use std::collections::HashMap;

use rmcp::ErrorData;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::schemars;
use rmcp::tool;
use rmcp::tool_router;

pub use nix::{NixError, build_package, develop, run_package};
pub use sandbox::{RunOptions, run_program};

/// Parameters for the `nix_build` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BuildParams {
  /// A nix package reference, for example `nixpkgs#hello`.
  #[schemars(
    description = "A nix package reference, for example `nixpkgs#hello`."
  )]
  pub package: String,
}

/// Parameters for the `nix_run` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RunParams {
  /// A nix package reference, for example `nixpkgs#hello`.
  #[schemars(
    description = "A nix package reference, for example `nixpkgs#hello`."
  )]
  pub package: String,
  /// The program to run from the built package, for example `hello`.
  #[schemars(
    description = "The program to run from the built package, for example `hello`."
  )]
  pub program: String,
  /// Arguments passed to the program.
  #[schemars(description = "Arguments passed to the program.")]
  #[serde(default)]
  pub args: Vec<String>,
  /// Additional environment variables set for the program.
  #[schemars(
    description = "Additional environment variables set for the program. The program runs in a sandbox with a clean environment, so these supplement the minimal base environment."
  )]
  #[serde(default)]
  pub env: HashMap<String, String>,
  /// The working directory for the program.
  #[schemars(
    description = "The working directory for the program. Inside the sandbox it must be visible, for example `/tmp`."
  )]
  #[serde(default)]
  pub cwd: Option<String>,
}

/// Parameters for the `nix_develop` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DevelopParams {
  /// A flake reference, for example `path:./.` or `github:foo/bar#some-devshell`.
  #[schemars(
    description = "A flake reference, for example `path:./.` or `github:foo/bar#some-devshell`. The dev shell defaults to `devShells.<system>.default`."
  )]
  pub flake: String,
  /// The command to run inside the dev shell, for example `cargo`.
  #[schemars(
    description = "The command to run inside the dev shell, for example `cargo`."
  )]
  pub command: String,
  /// Arguments passed to the command.
  #[schemars(description = "Arguments passed to the command.")]
  #[serde(default)]
  pub args: Vec<String>,
  /// Additional environment variables set for the command.
  #[schemars(
    description = "Additional environment variables set for the command. The command runs in a sandbox with the dev shell environment, so these supplement it."
  )]
  #[serde(default)]
  pub env: HashMap<String, String>,
  /// The working directory for the command.
  #[schemars(
    description = "The working directory for the command. Inside the sandbox it must be visible, for example `/tmp`."
  )]
  #[serde(default)]
  pub cwd: Option<String>,
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

  /// Build a nix package and run one of its programs, wrapped in a bubblewrap
  /// sandbox.
  #[tool(
    description = "Build a nix package and run one of its programs, wrapped in a bubblewrap sandbox."
  )]
  async fn nix_run(
    &self,
    Parameters(params): Parameters<RunParams>,
  ) -> Result<CallToolResult, ErrorData> {
    let RunParams {
      package,
      program,
      args,
      env,
      cwd,
    } = params;

    match tokio::task::spawn_blocking(move || {
      let options = RunOptions {
        args,
        env,
        cwd,
        sandbox: sandbox::sandbox_args(),
      };
      run_package(&package, &program, &options)
    })
    .await
    {
      Ok(Ok(output)) => {
        Ok(CallToolResult::success(vec![ContentBlock::text(output)]))
      }
      Ok(Err(error)) => Ok(CallToolResult::error(vec![ContentBlock::text(
        error.to_string(),
      )])),
      Err(join_error) => {
        Err(ErrorData::internal_error(join_error.to_string(), None))
      }
    }
  }

  /// Enter a nix dev shell and run one of its commands, wrapped in a bubblewrap
  /// sandbox.
  #[tool(
    description = "Enter a nix dev shell and run one of its commands, wrapped in a bubblewrap sandbox."
  )]
  async fn nix_develop(
    &self,
    Parameters(params): Parameters<DevelopParams>,
  ) -> Result<CallToolResult, ErrorData> {
    let DevelopParams {
      flake,
      command,
      args,
      env,
      cwd,
    } = params;

    match tokio::task::spawn_blocking(move || {
      let options = RunOptions {
        args,
        env,
        cwd,
        sandbox: sandbox::sandbox_args(),
      };
      develop(&flake, &command, &options)
    })
    .await
    {
      Ok(Ok(output)) => {
        Ok(CallToolResult::success(vec![ContentBlock::text(output)]))
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
