//! Wrappers around `nix` commands.

use std::collections::HashMap;
use std::process::Command;

use crate::sandbox;
use crate::sandbox::SandboxError;

/// Environment variables a dev shell must not override, matching `nix develop`.
const IGNORED_DEV_ENV: &[&str] = &[
  "BASHOPTS",
  "HOME",
  "NIX_BUILD_TOP",
  "NIX_ENFORCE_PURITY",
  "NIX_LOG_FD",
  "NIX_REMOTE",
  "PPID",
  "SHELLOPTS",
  "SSL_CERT_FILE",
  "TEMP",
  "TEMPDIR",
  "TERM",
  "TMP",
  "TMPDIR",
  "TZ",
  "UID",
];

/// Errors produced while building a nix package or entering a dev shell.
#[derive(Debug, thiserror::Error)]
pub enum NixError {
  /// Failed to spawn the `nix` binary.
  #[error("failed to run nix: {source}")]
  Spawn {
    /// The underlying I/O error.
    #[source]
    source: std::io::Error,
  },
  /// `nix build` exited with a non-zero status.
  #[error("nix build failed:\n{stderr}")]
  BuildFailed {
    /// The standard error output of the `nix` command.
    stderr: String,
  },
  /// `nix build` printed nothing to stdout.
  #[error("nix build produced no output")]
  EmptyOutput,
  /// `nix print-dev-env` exited with a non-zero status.
  #[error("failed to capture the dev shell environment:\n{stderr}")]
  DevelopEnv {
    /// The standard error output of the `nix` command.
    stderr: String,
  },
  /// The output of `nix print-dev-env` could not be parsed.
  #[error("failed to parse the dev shell environment")]
  DevelopEnvParse,
  /// Failed to run a program from the built package.
  #[error("failed to run the built package: {0}")]
  Run(#[source] SandboxError),
  /// Failed to set up the sandbox for a dev shell command.
  #[error("failed to set up the dev shell sandbox: {0}")]
  DevelopSandbox(#[source] SandboxError),
  /// The command run inside the dev shell exited with a non-zero status.
  #[error("command {program} failed:\n{stderr}")]
  CommandFailed {
    /// The command being run.
    program: String,
    /// The standard error output of the command.
    stderr: String,
  },
  /// The program or command is not in the `MCP_NIX_COMMANDS` allow list.
  #[error("command {command} is not allowed by MCP_NIX_COMMANDS")]
  CommandNotAllowed {
    /// The program or command being run.
    command: String,
  },
  /// `nix flake check` exited with a non-zero status.
  #[error("nix flake check failed:\n{stderr}")]
  CheckFailed {
    /// The standard error output of the `nix` command.
    stderr: String,
  },
}

/// Build the given nix package and return its nix store path.
///
/// When `show_trace` is set, `--show-trace` is passed so that nix evaluation
/// errors include the full stack trace.
pub fn build_package(
  package: &str,
  show_trace: bool,
) -> Result<String, NixError> {
  let output = Command::new("nix")
    .args(build_args(package, show_trace))
    .output()
    .map_err(|source| NixError::Spawn { source })?;

  if !output.status.success() {
    return Err(NixError::BuildFailed {
      stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    });
  }

  output
    .stdout
    .split(|byte| *byte == b'\n')
    .map(|line| String::from_utf8_lossy(line).trim().to_string())
    .find(|line| !line.is_empty())
    .ok_or(NixError::EmptyOutput)
}

/// Arguments for `nix build`, given a package reference.
fn build_args(package: &str, show_trace: bool) -> Vec<String> {
  let mut args = vec![
    "build".to_string(),
    "--no-link".to_string(),
    "--print-out-paths".to_string(),
    package.to_string(),
  ];
  if show_trace {
    args.push("--show-trace".to_string());
  }
  args
}

/// Build the given nix package and run one of its programs, returning the
/// program's standard output.
pub fn run_package(
  package: &str,
  program: &str,
  options: &sandbox::RunOptions,
) -> Result<String, NixError> {
  if !sandbox::command_allowed(program, &options.allowed_commands) {
    return Err(NixError::CommandNotAllowed {
      command: program.to_string(),
    });
  }

  let store_path = build_package(package, options.show_trace)?;
  sandbox::run_program(&store_path, program, options).map_err(NixError::Run)
}

/// The dev shell environment captured with `nix print-dev-env`.
struct DevShellEnv {
  /// Environment variables as name/value pairs.
  vars: Vec<(String, String)>,
  /// Shell functions as name/body pairs.
  bash_functions: Vec<(String, String)>,
}

/// The JSON output of `nix print-dev-env --json`.
#[derive(Debug, serde::Deserialize)]
struct PrintDevEnv {
  #[serde(default)]
  variables: HashMap<String, Variable>,
  #[serde(default)]
  bash_functions: HashMap<String, String>,
}

/// A variable in the output of `nix print-dev-env --json`.
#[derive(Debug, serde::Deserialize)]
struct Variable {
  #[serde(rename = "type")]
  ty: String,
  value: serde_json::Value,
}

impl Variable {
  /// Convert the variable to a string value, as bash would expand it.
  fn string_value(&self) -> Option<String> {
    match self.ty.as_str() {
      "var" | "exported" => self.value.as_str().map(str::to_string),
      "array" => Some(
        self
          .value
          .as_array()?
          .iter()
          .filter_map(serde_json::Value::as_str)
          .collect::<Vec<_>>()
          .join(" "),
      ),
      _ => None,
    }
  }
}

/// Capture the dev shell environment of the given flake with `nix print-dev-env`.
///
/// The command inherits the environment of the server, so that the user's nix
/// configuration applies.
fn capture_dev_env(
  nix: &str,
  flake: &str,
  options: &sandbox::RunOptions,
) -> Result<DevShellEnv, NixError> {
  let mut process = Command::new(nix);
  if let Some(cwd) = &options.cwd {
    process.current_dir(cwd);
  }
  process.args(print_dev_env_args(flake, options.show_trace));

  let output = process
    .output()
    .map_err(|source| NixError::Spawn { source })?;

  if !output.status.success() {
    return Err(NixError::DevelopEnv {
      stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    });
  }

  parse_dev_env(&output.stdout)
}

/// Arguments for `nix print-dev-env`, given a flake reference.
fn print_dev_env_args(flake: &str, show_trace: bool) -> Vec<String> {
  let mut args = vec![
    "print-dev-env".to_string(),
    flake.to_string(),
    "--json".to_string(),
  ];
  if show_trace {
    args.push("--show-trace".to_string());
  }
  args
}

fn parse_dev_env(stdout: &[u8]) -> Result<DevShellEnv, NixError> {
  let env: PrintDevEnv =
    serde_json::from_slice(stdout).map_err(|_| NixError::DevelopEnvParse)?;

  let mut vars = Vec::new();
  for (name, variable) in env.variables {
    if IGNORED_DEV_ENV.contains(&name.as_str()) {
      continue;
    }
    let Some(value) = variable.string_value() else {
      continue;
    };
    vars.push((name, value));
  }

  Ok(DevShellEnv {
    vars,
    bash_functions: env.bash_functions.into_iter().collect(),
  })
}

/// Build the command that runs in the dev shell.
///
/// When the shell defines functions or a shell hook, the command runs through a
/// bash wrapper that defines the functions, runs the `shellHook` and then
/// `exec`s the command. The command itself is never interpreted by the shell.
fn dev_command(
  bash: &str,
  command: &str,
  args: &[String],
  env: &DevShellEnv,
) -> (String, Vec<String>) {
  let has_hook = env.vars.iter().any(|(name, _)| name == "shellHook");
  if env.bash_functions.is_empty() && !has_hook {
    return (command.to_string(), args.to_vec());
  }

  let mut script = String::new();
  for (name, body) in &env.bash_functions {
    script.push_str(&format!("{name} ()\n{{\n{body}\n}}\n"));
  }
  script.push_str("eval \"${shellHook:-}\"\n");
  script.push_str("exec \"$@\"\n");

  let mut argv = vec![
    "-c".to_string(),
    script,
    bash.to_string(),
    command.to_string(),
  ];
  argv.extend(args.iter().cloned());

  (bash.to_string(), argv)
}

/// Enter the dev shell of the given flake and run a command in it, returning
/// the command's standard output.
///
/// The dev shell environment is captured with `nix print-dev-env`, which
/// inherits the environment of the server so that the user's nix configuration
/// applies. The command then runs with that environment, optionally wrapped in
/// the configured bubblewrap sandbox; the dev shell `shellHook` (and any shell
/// functions) run inside the sandbox before the command.
pub fn develop(
  flake: &str,
  command: &str,
  options: &sandbox::RunOptions,
) -> Result<String, NixError> {
  if !sandbox::command_allowed(command, &options.allowed_commands) {
    return Err(NixError::CommandNotAllowed {
      command: command.to_string(),
    });
  }

  let nix = sandbox::resolve_on_path("nix")
    .map_err(|source| NixError::Spawn { source })?;
  let bash = sandbox::resolve_on_path("bash")
    .map_err(|source| NixError::Spawn { source })?;

  let dev_env = capture_dev_env(&nix, flake, options)?;

  let mut vars = dev_env.vars;
  vars.push(("SHELL".to_string(), bash.clone()));
  let env = DevShellEnv {
    vars,
    bash_functions: dev_env.bash_functions,
  };

  let (effective_command, effective_args) =
    dev_command(&bash, command, &options.args, &env);

  let argv = sandbox::sandboxed_argv(
    &effective_command,
    &effective_args,
    options,
    &env.vars,
  )
  .map_err(NixError::DevelopSandbox)?;

  let overlay = sandbox::merged_env(&env.vars, options, false);

  sandbox::run_command_argv(&argv, options, &overlay, command).map_err(
    |error| match error {
      SandboxError::Failed { stderr, .. } => NixError::CommandFailed {
        program: command.to_string(),
        stderr,
      },
      other => NixError::DevelopSandbox(other),
    },
  )
}

/// Options for `nix flake check`.
#[derive(Debug, Default)]
pub struct CheckOptions {
  /// Check the flake's outputs for all systems, not just the current one.
  pub all_systems: bool,
  /// Only check that the flake evaluates, without building any derivations.
  pub no_build: bool,
  /// Pass `--show-trace` so that nix evaluation errors include the full stack
  /// trace.
  pub show_trace: bool,
}

/// Check the given flake with `nix flake check` and return its standard output.
pub fn check_flake(
  flake: &str,
  options: &CheckOptions,
) -> Result<String, NixError> {
  let output = Command::new("nix")
    .args(check_args(flake, options))
    .output()
    .map_err(|source| NixError::Spawn { source })?;

  if !output.status.success() {
    return Err(NixError::CheckFailed {
      stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    });
  }

  Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Arguments for `nix flake check`, given a flake reference and options.
fn check_args(flake: &str, options: &CheckOptions) -> Vec<String> {
  let mut args =
    vec!["flake".to_string(), "check".to_string(), flake.to_string()];
  if options.all_systems {
    args.push("--all-systems".to_string());
  }
  if options.no_build {
    args.push("--no-build".to_string());
  }
  if options.show_trace {
    args.push("--show-trace".to_string());
  }
  args
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn build_args_pass_no_flags_by_default() {
    assert_eq!(
      build_args("nixpkgs#hello", false),
      vec!["build", "--no-link", "--print-out-paths", "nixpkgs#hello"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
    );
  }

  #[test]
  fn build_args_include_show_trace_when_set() {
    let args = build_args("nixpkgs#hello", true);

    assert_eq!(args.last().map(String::as_str), Some("--show-trace"));
    assert_eq!(
      args[..args.len().saturating_sub(1)],
      vec!["build", "--no-link", "--print-out-paths", "nixpkgs#hello"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
    );
  }

  #[test]
  fn print_dev_env_args_pass_no_flags_by_default() {
    assert_eq!(
      print_dev_env_args("path:./.", false),
      vec!["print-dev-env", "path:./.", "--json"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
    );
  }

  #[test]
  fn print_dev_env_args_include_show_trace_when_set() {
    let args = print_dev_env_args("path:./.", true);

    assert_eq!(args.last().map(String::as_str), Some("--show-trace"));
    assert_eq!(
      args[..args.len().saturating_sub(1)],
      vec!["print-dev-env", "path:./.", "--json"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
    );
  }

  #[test]
  fn check_args_pass_no_flags_by_default() {
    assert_eq!(
      check_args("path:./.", &CheckOptions::default()),
      vec!["flake", "check", "path:./."]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
    );
  }

  #[test]
  fn check_args_add_each_flag_independently() {
    let all_systems = CheckOptions {
      all_systems: true,
      ..Default::default()
    };
    assert!(
      check_args("path:./.", &all_systems)
        .contains(&"--all-systems".to_string())
    );

    let no_build = CheckOptions {
      no_build: true,
      ..Default::default()
    };
    assert!(
      check_args("path:./.", &no_build).contains(&"--no-build".to_string())
    );

    let show_trace = CheckOptions {
      show_trace: true,
      ..Default::default()
    };
    assert!(
      check_args("path:./.", &show_trace).contains(&"--show-trace".to_string())
    );
  }

  #[test]
  fn check_args_add_all_flags() {
    let options = CheckOptions {
      all_systems: true,
      no_build: true,
      show_trace: true,
    };
    assert_eq!(
      check_args("path:./.", &options),
      vec![
        "flake",
        "check",
        "path:./.",
        "--all-systems",
        "--no-build",
        "--show-trace"
      ]
      .into_iter()
      .map(String::from)
      .collect::<Vec<_>>()
    );
  }
}
