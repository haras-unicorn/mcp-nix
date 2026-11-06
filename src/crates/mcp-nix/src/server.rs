//! The MCP server and the parameters of its tools.

use std::collections::HashMap;
use std::path::PathBuf;

use rmcp::ErrorData;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::schemars;
use rmcp::tool;
use rmcp::tool_router;

use crate::nix::{
  BuildGuard, CheckOptions, LogOptions, NixError, build_package, check_flake,
  develop, fetch_log, run_package,
};
use crate::sandbox::{RunOptions, confine_working_directory};

/// Settings of the [`NixServer`], chosen by whoever runs it.
#[derive(Debug, Clone)]
pub struct ServerSettings {
  /// What keeps the `nix` commands from building derivations the caller must
  /// not build.
  pub guard: BuildGuard,
  /// Bubblewrap arguments for the programs `nix_run` and `nix_develop` run, or
  /// `None` to run them unsandboxed.
  pub sandbox: Option<Vec<String>>,
  /// The command that runs bubblewrap, as a list of words. Empty means plain
  /// `bwrap`.
  pub bwrap_command: Vec<String>,
  /// The command file names `nix_run` and `nix_develop` may execute. Empty
  /// means any command is allowed.
  pub allowed_commands: Vec<String>,
  /// The number of lines per `nix_log` page when the caller gives no limit.
  pub log_limit: usize,
  /// The directory the working directory of `nix_run` and `nix_develop` has
  /// to be in, after links are followed, and that a relative one is taken
  /// from. It is also where a program runs when it is not sandboxed and the
  /// caller gives no working directory. The guard decides where the `nix`
  /// commands run, see [`BuildGuard::in_directory`].
  pub root: PathBuf,
}

/// MCP server that provides nix tooling.
#[derive(Debug, Clone)]
pub struct NixServer {
  settings: ServerSettings,
}

impl NixServer {
  /// Create a server with the given settings.
  pub fn new(settings: ServerSettings) -> Self {
    Self { settings }
  }

  fn run_options(
    &self,
    args: Vec<String>,
    env: HashMap<String, String>,
    cwd: Option<String>,
    show_trace: bool,
  ) -> Result<RunOptions, NixError> {
    let cwd = match cwd {
      Some(cwd) => {
        confine_working_directory(&cwd, &self.settings.root).map(Some)
      }
      None if self.settings.sandbox.is_none() => {
        confine_working_directory(".", &self.settings.root).map(Some)
      }
      None => Ok(None),
    }
    .map_err(NixError::WorkingDirectory)?;

    Ok(RunOptions {
      args,
      env,
      cwd,
      sandbox: self.settings.sandbox.clone(),
      bwrap: self.settings.bwrap_command.clone(),
      allowed_commands: self.settings.allowed_commands.clone(),
      show_trace,
    })
  }
}

#[tool_router(server_handler)]
impl NixServer {
  /// Build a nix package and return its nix store path.
  #[tool]
  async fn nix_build(
    &self,
    Parameters(params): Parameters<BuildParams>,
  ) -> Result<CallToolResult, ErrorData> {
    let BuildParams {
      package,
      show_trace,
    } = params;

    let guard = self.settings.guard.clone();

    tool_result(
      "nix_build",
      tokio::task::spawn_blocking(move || {
        build_package(&guard, &package, show_trace)
      })
      .await,
    )
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
    let options = match self.run_options(args, env, cwd, show_trace) {
      Ok(options) => options,
      Err(error) => return tool_result("nix_run", Ok(Err(error))),
    };
    let guard = self.settings.guard.clone();

    tool_result(
      "nix_run",
      tokio::task::spawn_blocking(move || {
        run_package(&guard, &package, &program, &options)
      })
      .await,
    )
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
    let options = match self.run_options(args, env, cwd, show_trace) {
      Ok(options) => options,
      Err(error) => return tool_result("nix_develop", Ok(Err(error))),
    };
    let guard = self.settings.guard.clone();

    tool_result(
      "nix_develop",
      tokio::task::spawn_blocking(move || {
        develop(&guard, &flake, &command, &options)
      })
      .await,
    )
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

    let guard = self.settings.guard.clone();

    tool_result(
      "nix_check",
      tokio::task::spawn_blocking(move || {
        check_flake(&guard, &flake, &options)
      })
      .await,
    )
  }

  /// Fetch and paginate the build log of a nix package or store path.
  #[tool]
  async fn nix_log(
    &self,
    Parameters(params): Parameters<LogParams>,
  ) -> Result<CallToolResult, ErrorData> {
    let LogParams {
      package,
      offset,
      limit,
      from_end,
    } = params;
    let options = LogOptions {
      offset,
      limit: limit.unwrap_or(self.settings.log_limit),
      from_end,
    };

    let guard = self.settings.guard.clone();

    tool_result(
      "nix_log",
      tokio::task::spawn_blocking(move || {
        fetch_log(&guard, &package, &options)
      })
      .await,
    )
  }
}

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
  /// The working directory for the program. It has to be inside the root of
  /// the server, and a relative one is taken from there.
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
  /// The working directory for the command. It has to be inside the root of
  /// the server, and a relative one is taken from there.
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

/// Parameters for the `nix_log` tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LogParams {
  /// A nix package reference or store path, for example `nixpkgs#hello` or the
  /// failing `/nix/store/...-.drv` printed by a build error.
  pub package: String,
  /// The 0-based line the page starts at; counted from the end of the log when
  /// `from_end` is set.
  #[serde(default)]
  pub offset: usize,
  /// The number of lines per page. Defaults to the `log_limit` setting of the
  /// server.
  #[serde(default)]
  pub limit: Option<usize>,
  /// Return a window at the end of the log instead of the beginning: the page
  /// is `total - offset - limit..total - offset` instead of
  /// `offset..offset + limit`. An `offset` of `0` returns the last `limit`
  /// lines, and increasing `offset` walks backwards through the log.
  #[serde(default)]
  pub from_end: bool,
}

/// Turn the outcome of a blocking tool task into an MCP result, logging how it
/// went. A failing tool is a result the caller can read and not a protocol
/// error.
fn tool_result(
  tool: &str,
  joined: Result<Result<String, NixError>, tokio::task::JoinError>,
) -> Result<CallToolResult, ErrorData> {
  match joined {
    Ok(Ok(output)) => {
      tracing::info!(tool, "tool succeeded");
      Ok(CallToolResult::success(vec![ContentBlock::text(output)]))
    }
    Ok(Err(error)) => {
      tracing::warn!(tool, %error, "tool failed");
      Ok(CallToolResult::error(vec![ContentBlock::text(
        error.to_string(),
      )]))
    }
    Err(join_error) => {
      tracing::error!(tool, %join_error, "tool task did not finish");
      Err(ErrorData::internal_error(join_error.to_string(), None))
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn settings() -> ServerSettings {
    ServerSettings {
      guard: BuildGuard::unguarded(),
      sandbox: None,
      bwrap_command: vec!["nsenter".to_string(), "bwrap".to_string()],
      allowed_commands: vec!["hello".to_string()],
      log_limit: 7,
      root: std::env::temp_dir(),
    }
  }

  #[test]
  fn run_options_carry_the_server_settings() {
    let server = NixServer::new(settings());

    let options = server
      .run_options(vec!["--flag".to_string()], HashMap::new(), None, true)
      .unwrap();

    assert_eq!(options.args, vec!["--flag".to_string()]);
    assert_eq!(options.allowed_commands, vec!["hello".to_string()]);
    assert_eq!(
      options.bwrap,
      vec!["nsenter".to_string(), "bwrap".to_string()]
    );
    assert_eq!(options.sandbox, None);
    assert!(options.show_trace);
  }

  #[test]
  fn run_options_refuse_a_working_directory_outside_the_root() {
    let server = NixServer::new(settings());

    let result = server.run_options(
      Vec::new(),
      HashMap::new(),
      Some("/".to_string()),
      false,
    );

    assert!(matches!(result, Err(NixError::WorkingDirectory(_))));
  }

  #[test]
  fn an_unsandboxed_program_runs_in_the_root_by_default() {
    let server = NixServer::new(settings());
    let root = std::env::temp_dir().canonicalize().unwrap();

    let options = server
      .run_options(Vec::new(), HashMap::new(), None, false)
      .unwrap();

    assert_eq!(options.cwd, Some(root.to_string_lossy().into_owned()));
  }

  #[test]
  fn a_sandboxed_program_has_no_working_directory_by_default() {
    let server = NixServer::new(ServerSettings {
      sandbox: Some(vec!["--unshare-all".to_string()]),
      ..settings()
    });

    let options = server
      .run_options(Vec::new(), HashMap::new(), None, false)
      .unwrap();

    assert_eq!(options.cwd, None);
  }

  #[test]
  fn run_options_resolve_a_working_directory_inside_the_root() {
    let server = NixServer::new(settings());
    let root = std::env::temp_dir().canonicalize().unwrap();

    let options = server
      .run_options(Vec::new(), HashMap::new(), Some(".".to_string()), false)
      .unwrap();

    assert_eq!(options.cwd, Some(root.to_string_lossy().into_owned()));
  }

  #[test]
  fn tool_result_reports_output_as_success() {
    let result =
      tool_result("nix_build", Ok(Ok("/nix/store/x".to_string()))).unwrap();

    assert_eq!(result.is_error, Some(false));
  }

  #[test]
  fn tool_result_reports_nix_errors_as_tool_errors() {
    let result =
      tool_result("nix_build", Ok(Err(NixError::EmptyOutput))).unwrap();

    assert_eq!(result.is_error, Some(true));
  }
}
