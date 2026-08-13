//! Sandboxing of programs run from built nix packages and dev shells.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

pub use crate::sandbox_defaults::{BASE_ENV, DEFAULT_SANDBOX_ARGS};

/// Errors produced while running a program from a built nix package or a dev
/// shell command.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
  /// Failed to spawn the sandboxed program.
  #[error("failed to run {program}: {source}")]
  Spawn {
    /// The program being run.
    program: String,
    /// The underlying I/O error.
    #[source]
    source: std::io::Error,
  },
  /// Failed to spawn bubblewrap.
  #[error(
    "failed to spawn the sandbox: {source} (is bubblewrap installed and MCP_NIX_SANDBOX set?)"
  )]
  SandboxSpawn {
    /// The underlying I/O error.
    #[source]
    source: std::io::Error,
  },
  /// The program exited with a non-zero status.
  #[error("{program} failed:\n{stderr}")]
  Failed {
    /// The program being run.
    program: String,
    /// The standard error output of the program.
    stderr: String,
  },
}

/// Parse the value of the `MCP_NIX_SANDBOX` environment variable.
///
/// - an unset value (`None`) yields the [`DEFAULT_SANDBOX_ARGS`];
/// - a blank value disables the sandbox;
/// - any other value is split on whitespace and used as the bubblewrap
///   arguments, replacing the default set entirely.
pub fn parse_sandbox_args(value: Option<&str>) -> Option<Vec<String>> {
  match value.map(str::trim) {
    Some("") => None,
    Some(value) => Some(split_args(value)),
    None => Some(split_args(DEFAULT_SANDBOX_ARGS)),
  }
}

fn split_args(value: &str) -> Vec<String> {
  value
    .split_whitespace()
    .map(str::to_string)
    .collect::<Vec<String>>()
}

/// The `MCP_NIX_SANDBOX` environment variable of the current process.
pub fn sandbox_args() -> Option<Vec<String>> {
  parse_sandbox_args(std::env::var("MCP_NIX_SANDBOX").ok().as_deref())
}

/// Parse the value of the `MCP_NIX_COMMANDS` environment variable.
///
/// The variable holds a comma-separated list of command file names that may be
/// executed. An unset or blank value yields an empty list, meaning no
/// restriction; entries are trimmed and empty entries are skipped.
pub fn parse_allowed_commands(value: Option<&str>) -> Vec<String> {
  value
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(|value| {
      value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
    })
    .unwrap_or_default()
}

/// The `MCP_NIX_COMMANDS` environment variable of the current process.
pub fn allowed_commands() -> Vec<String> {
  parse_allowed_commands(std::env::var("MCP_NIX_COMMANDS").ok().as_deref())
}

/// The executable file name of a command path, for example `cargo` for
/// `/nix/store/.../bin/cargo`.
fn executable_name(command: &str) -> &str {
  Path::new(command)
    .file_name()
    .and_then(|name| name.to_str())
    .unwrap_or(command)
}

/// Whether the given command is allowed by the `MCP_NIX_COMMANDS` allow list.
///
/// An empty allow list allows any command. Otherwise the executable file name
/// of `command` must match one of the entries.
pub fn command_allowed(command: &str, allowed: &[String]) -> bool {
  allowed.is_empty()
    || allowed
      .iter()
      .any(|entry| entry.as_str() == executable_name(command))
}

/// Options for running a program from a built nix package or a dev shell
/// command.
#[derive(Debug, Default)]
pub struct RunOptions {
  /// Arguments passed to the program.
  pub args: Vec<String>,
  /// Additional environment variables set for the program.
  pub env: HashMap<String, String>,
  /// The working directory for the program.
  pub cwd: Option<String>,
  /// Bubblewrap arguments, or `None` to run the program directly.
  pub sandbox: Option<Vec<String>>,
  /// The `MCP_NIX_COMMANDS` allow list: command file names that may be
  /// executed. Empty means any command is allowed.
  pub allowed_commands: Vec<String>,
}

/// Compute the environment set inside the sandbox for a command.
///
/// - `extra` provides an additional environment, for example a captured dev
///   shell environment; a `PATH` in it is merged with the base `PATH`;
/// - the custom environment in [`RunOptions::env`] always wins.
///
/// When `base` is true the minimal [`BASE_ENV`] is included first. When `base`
/// is false only `extra` and the custom environment are included, used to
/// overlay a dev shell environment on an unsandboxed command that otherwise
/// inherits the environment of the server.
pub fn merged_env(
  extra: &[(String, String)],
  options: &RunOptions,
  base: bool,
) -> Vec<(String, String)> {
  let mut env: Vec<(String, String)> = if base {
    BASE_ENV
      .iter()
      .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
      .collect()
  } else {
    Vec::new()
  };

  for (key, value) in extra {
    if key == "PATH" && base {
      let base_path = env
        .iter()
        .find(|(existing, _)| existing == key)
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
      env.push(("PATH".to_string(), format!("{value}:{base_path}")));
    } else {
      env.push((key.clone(), value.clone()));
    }
  }

  for (key, value) in &options.env {
    env.retain(|(existing, _)| existing != key);
    env.push((key.clone(), value.clone()));
  }

  env
}

/// Resolve an executable to an absolute path using the current process's
/// `PATH`.
///
/// The program runs with a cleared environment, so executables must be located
/// before their own `PATH` is cleared.
pub fn resolve_on_path(program: &str) -> Result<String, std::io::Error> {
  let path = std::env::var_os("PATH").ok_or_else(|| {
    std::io::Error::new(std::io::ErrorKind::NotFound, "PATH is not set")
  })?;

  for dir in std::env::split_paths(&path) {
    let candidate = dir.join(program);
    if is_executable(&candidate) {
      return Ok(candidate.to_string_lossy().into_owned());
    }
  }

  Err(std::io::Error::new(
    std::io::ErrorKind::NotFound,
    format!("{program} not found on PATH"),
  ))
}

fn is_executable(path: &std::path::Path) -> bool {
  use std::os::unix::fs::PermissionsExt;
  path.metadata().is_ok_and(|metadata| {
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
  })
}

fn resolve_bwrap() -> Result<String, SandboxError> {
  resolve_on_path("bwrap")
    .map_err(|source| SandboxError::SandboxSpawn { source })
}

/// Build the argument vector for running `command` with `args`, wrapped in the
/// configured sandbox.
///
/// When [`RunOptions::sandbox`] is `Some`, `bwrap` is resolved to an absolute
/// path and prepended together with the sandbox arguments, `--clearenv`,
/// `--setenv` pairs for [`merged_env`], an optional `--chdir`, the command and
/// its arguments. When it is `None`, the command and its arguments are returned
/// directly.
pub fn sandboxed_argv(
  command: &str,
  args: &[String],
  options: &RunOptions,
  extra_env: &[(String, String)],
) -> Result<Vec<String>, SandboxError> {
  match &options.sandbox {
    Some(bwrap_args) => {
      let bwrap = resolve_bwrap()?;
      let mut argv = Vec::new();
      argv.push(bwrap);
      argv.extend(bwrap_args.iter().cloned());
      argv.push("--clearenv".to_string());
      for (key, value) in merged_env(extra_env, options, true) {
        argv.push("--setenv".to_string());
        argv.push(key);
        argv.push(value);
      }
      if let Some(cwd) = &options.cwd {
        argv.push("--chdir".to_string());
        argv.push(cwd.clone());
      }
      argv.push(command.to_string());
      argv.extend(args.iter().cloned());
      Ok(argv)
    }
    None => {
      let mut argv = Vec::new();
      argv.push(command.to_string());
      argv.extend(args.iter().cloned());
      Ok(argv)
    }
  }
}

/// Run the given argument vector, optionally wrapped in a bubblewrap sandbox,
/// and return its standard output.
///
/// When sandboxed the environment is controlled by the `--clearenv`/`--setenv`
/// arguments in `argv`. When unsandboxed the command inherits the environment
/// of the server, with `overlay` applied on top.
pub fn run_command_argv(
  argv: &[String],
  options: &RunOptions,
  overlay: &[(String, String)],
  program: &str,
) -> Result<String, SandboxError> {
  let sandboxed = options.sandbox.is_some();

  let mut process = Command::new(&argv[0]);
  if let Some(cwd) = &options.cwd {
    process.current_dir(cwd);
  }
  if !sandboxed {
    for (key, value) in overlay {
      process.env(key, value);
    }
  }
  process.args(&argv[1..]);

  let output = process.output().map_err(|source| {
    if sandboxed {
      SandboxError::SandboxSpawn { source }
    } else {
      SandboxError::Spawn {
        program: program.to_string(),
        source,
      }
    }
  })?;

  if !output.status.success() {
    return Err(SandboxError::Failed {
      program: program.to_string(),
      stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    });
  }

  Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run an arbitrary command, optionally wrapped in a bubblewrap sandbox, and
/// return its standard output.
///
/// When sandboxed the command runs with a clean environment: only [`BASE_ENV`]
/// plus the variables in [`RunOptions::env`]. When unsandboxed it inherits the
/// environment of the server, with the variables in [`RunOptions::env`] applied
/// on top.
pub fn run_sandboxed(
  command: &str,
  args: &[String],
  options: &RunOptions,
) -> Result<String, SandboxError> {
  let argv = sandboxed_argv(command, args, options, &[])?;
  let overlay: Vec<(String, String)> = options
    .env
    .iter()
    .map(|(key, value)| (key.clone(), value.clone()))
    .collect();
  run_command_argv(&argv, options, &overlay, command)
}

/// Run a program from a built nix package, optionally wrapped in a bubblewrap
/// sandbox, and return its standard output.
///
/// The program runs with a clean environment when sandboxed: only [`BASE_ENV`]
/// plus the variables in [`RunOptions::env`]. When unsandboxed it inherits the
/// environment of the server, with the variables in [`RunOptions::env`] applied
/// on top.
pub fn run_program(
  store_path: &str,
  program: &str,
  options: &RunOptions,
) -> Result<String, SandboxError> {
  let bin = format!("{store_path}/bin/{program}");
  run_sandboxed(&bin, &options.args, options)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sandbox_args_use_defaults_when_unset() {
    assert_eq!(
      parse_sandbox_args(None),
      parse_sandbox_args(Some(DEFAULT_SANDBOX_ARGS))
    );
  }

  #[test]
  fn sandbox_args_defaults_restrict_network() {
    let args = parse_sandbox_args(None).unwrap_or_default();
    assert!(args.contains(&"--unshare-net".to_string()));
  }

  #[test]
  fn sandbox_args_disabled_when_blank() {
    assert_eq!(parse_sandbox_args(Some("   ")), None);
  }

  #[test]
  fn sandbox_args_splits_on_whitespace() {
    assert_eq!(
      parse_sandbox_args(Some("--die-with-parent --ro-bind /nix /nix")),
      Some(vec![
        "--die-with-parent".to_string(),
        "--ro-bind".to_string(),
        "/nix".to_string(),
        "/nix".to_string(),
      ])
    );
  }

  #[test]
  fn allowed_commands_empty_when_unset_or_blank() {
    assert!(parse_allowed_commands(None).is_empty());
    assert!(parse_allowed_commands(Some("   ")).is_empty());
    assert!(parse_allowed_commands(Some(",, ,")).is_empty());
  }

  #[test]
  fn allowed_commands_splits_on_commas() {
    assert_eq!(
      parse_allowed_commands(Some("hello, cargo,  make")),
      vec!["hello".to_string(), "cargo".to_string(), "make".to_string()]
    );
  }

  #[test]
  fn command_allowed_matches_executable_file_name() {
    assert!(command_allowed("hello", &[]));
    assert!(command_allowed("hello", &["hello".to_string()]));
    assert!(command_allowed(
      "/nix/store/abc/bin/cargo",
      &["cargo".to_string()]
    ));
    assert!(!command_allowed("hello", &["hello2".to_string()]));
    assert!(!command_allowed(
      "/nix/store/abc/bin/cargo",
      &["make".to_string()]
    ));
  }
}
