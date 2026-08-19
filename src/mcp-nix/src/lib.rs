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

pub use nix::{
  CheckOptions, NixError, build_package, check_flake, develop, run_package,
};
pub use sandbox::{RunOptions, run_program};

/// Parameters for the `nix_build` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BuildParams {
  /// A nix package reference, for example `nixpkgs#hello`.
  pub package: String,
  /// Pass `--show-trace` to `nix build` so that evaluation errors include the
  /// full stack trace.
  #[serde(default)]
  pub show_trace: bool,
}

/// Parameters for the `nix_run` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RunParams {
  /// A nix package reference, for example `nixpkgs#hello`.
  pub package: String,
  /// Pass `--show-trace` to the `nix build` step so that evaluation errors
  /// include the full stack trace.
  #[serde(default)]
  pub show_trace: bool,
  /// The program to run from the built package, for example `hello`.
  pub program: String,
  /// Arguments passed to the program.
  #[serde(default)]
  pub args: Vec<String>,
  /// Additional environment variables set for the program.
  #[serde(default)]
  pub env: HashMap<String, String>,
  /// The working directory for the program.
  #[serde(default)]
  pub cwd: Option<String>,
}

/// Parameters for the `nix_develop` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DevelopParams {
  /// A flake reference, for example `path:./.` or `github:foo/bar#some-devshell`.
  pub flake: String,
  /// Pass `--show-trace` to `nix print-dev-env` so that evaluation errors
  /// include the full stack trace.
  #[serde(default)]
  pub show_trace: bool,
  /// The command to run inside the dev shell, for example `cargo`.
  pub command: String,
  /// Arguments passed to the command.
  #[serde(default)]
  pub args: Vec<String>,
  /// Additional environment variables set for the command.
  #[serde(default)]
  pub env: HashMap<String, String>,
  /// The working directory for the command.
  #[serde(default)]
  pub cwd: Option<String>,
}

/// Parameters for the `nix_check` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CheckParams {
  /// A flake reference, for example `path:./.` or `github:foo/bar`.
  pub flake: String,
  /// Check the flake's outputs for all systems, not just the current one.
  #[serde(default)]
  pub all_systems: bool,
  /// Only check that the flake evaluates, without building any derivations.
  #[serde(default)]
  pub no_build: bool,
  /// Pass `--show-trace` to `nix flake check` so that evaluation errors include
  /// the full stack trace.
  #[serde(default)]
  pub show_trace: bool,
}

/// MCP server that provides nix tooling.
#[derive(Debug, Clone)]
pub struct NixServer;

#[tool_router(server_handler)]
impl NixServer {
  /// Build a nix package and return its nix store path.
  #[tool]
  async fn nix_build(
    &self,
    Parameters(params): Parameters<BuildParams>,
  ) -> Result<CallToolResult, ErrorData> {
    let package = params.package;
    let show_trace = params.show_trace;

    match tokio::task::spawn_blocking(move || {
      build_package(&package, show_trace)
    })
    .await
    {
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
  #[tool]
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
      show_trace,
    } = params;

    match tokio::task::spawn_blocking(move || {
      let options = RunOptions {
        args,
        env,
        cwd,
        sandbox: sandbox::sandbox_args(),
        allowed_commands: sandbox::allowed_commands(),
        show_trace,
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
  #[tool]
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
      show_trace,
    } = params;

    match tokio::task::spawn_blocking(move || {
      let options = RunOptions {
        args,
        env,
        cwd,
        sandbox: sandbox::sandbox_args(),
        allowed_commands: sandbox::allowed_commands(),
        show_trace,
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

  /// Check a nix flake for errors with `nix flake check`.
  #[tool]
  async fn nix_check(
    &self,
    Parameters(params): Parameters<CheckParams>,
  ) -> Result<CallToolResult, ErrorData> {
    let CheckParams {
      flake,
      all_systems,
      no_build,
      show_trace,
    } = params;

    let options = CheckOptions {
      all_systems,
      no_build,
      show_trace,
    };

    match tokio::task::spawn_blocking(move || check_flake(&flake, &options))
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
