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
  /// `nix log` exited with a non-zero status.
  #[error("nix log failed:\n{stderr}")]
  LogFailed {
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

/// Options for `nix log`.
#[derive(Debug, Default)]
pub struct LogOptions {
  /// The 0-based line the page starts at; counted from the end of the log when
  /// `from_end` is set.
  pub offset: usize,
  /// The number of lines per page.
  pub limit: usize,
  /// Return a window at the end of the log instead of the beginning: the page
  /// is `total - offset - limit..total - offset` instead of
  /// `offset..offset + limit`.
  pub from_end: bool,
}

/// Fetch the build log of the given package or store path and return one page
/// of it.
///
/// Build logs can be extremely long (for example nixos tests), so the log is
/// paginated server-side: the page is a window of lines ending with a footer
/// describing the window and the offset of the next page.
pub fn fetch_log(
  package: &str,
  options: &LogOptions,
) -> Result<String, NixError> {
  let output = Command::new("nix")
    .args(["log", package])
    .output()
    .map_err(|source| NixError::Spawn { source })?;

  if !output.status.success() {
    return Err(NixError::LogFailed {
      stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    });
  }

  let log = String::from_utf8_lossy(&output.stdout);
  if log.trim().is_empty() {
    return Ok(format!("no build log available for {package}"));
  }

  let lines = log.lines().map(str::to_string).collect::<Vec<_>>();
  let page = paginate(&lines, options.offset, options.limit, options.from_end);

  let mut result = page.lines.join("\n");
  if !result.is_empty() {
    result.push('\n');
  }
  result.push_str(&describe_page(
    page.start,
    page.end,
    page.total,
    page.next_offset,
  ));
  Ok(result)
}

/// A window into a build log.
struct LogPage {
  /// The lines of the window.
  lines: Vec<String>,
  /// The index of the first line of the window.
  start: usize,
  /// The index one past the last line of the window.
  end: usize,
  /// The total number of lines in the log.
  total: usize,
  /// The `offset` to request for the next page, or `None` when the page
  /// reaches the end of the log.
  next_offset: Option<usize>,
}

/// Compute the window of a build log for the given pagination options.
///
/// Without `from_end` the window is `offset..offset + limit`, with `offset`
/// clamped to the end of the log, producing an empty page when it is past the
/// end. With `from_end` set the window is `total - offset - limit..total -
/// offset`, starting from the end of the log: an `offset` of `0` returns the
/// last `limit` lines and increasing `offset` walks backwards through the log.
fn paginate(
  lines: &[String],
  offset: usize,
  limit: usize,
  from_end: bool,
) -> LogPage {
  let total = lines.len();
  let (start, end, next_offset) = if from_end {
    let end = total.saturating_sub(offset);
    let start = end.saturating_sub(limit);
    let next_offset = if start == 0 {
      None
    } else {
      Some(offset.saturating_add(limit))
    };
    (start, end, next_offset)
  } else {
    let end = offset.saturating_add(limit).min(total);
    let start = offset.min(total);
    let next_offset = if end >= total { None } else { Some(end) };
    (start, end, next_offset)
  };

  LogPage {
    lines: lines[start..end].to_vec(),
    start,
    end,
    total,
    next_offset,
  }
}

/// Describe a log page: its window of lines and how to navigate to the next
/// page. The footer is appended to the raw page lines.
fn describe_page(
  start: usize,
  end: usize,
  total: usize,
  next_offset: Option<usize>,
) -> String {
  if start == end {
    return format!("[nix_log] end of log ({total} total lines)");
  }
  let window = format!("lines {start}..{} of {total}", end.saturating_sub(1));
  let navigation = match next_offset {
    Some(offset) => format!("next offset: {offset}"),
    None => "end of log".to_string(),
  };
  format!("[nix_log] {window} · {navigation}")
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

  #[test]
  fn paginate_returns_first_page_by_default() {
    let lines = (0..250).map(|i| i.to_string()).collect::<Vec<_>>();
    let page = paginate(&lines, 0, 100, false);

    assert_eq!(page.start, 0);
    assert_eq!(page.end, 100);
    assert_eq!(page.total, 250);
    assert_eq!(page.next_offset, Some(100));
    assert_eq!(page.lines.len(), 100);
    assert_eq!(page.lines.first().map(String::as_str), Some("0"));
    assert_eq!(page.lines.last().map(String::as_str), Some("99"));
  }

  #[test]
  fn paginate_returns_middle_page() {
    let lines = (0..250).map(|i| i.to_string()).collect::<Vec<_>>();
    let page = paginate(&lines, 100, 100, false);

    assert_eq!(page.start, 100);
    assert_eq!(page.end, 200);
    assert_eq!(page.next_offset, Some(200));
    assert_eq!(page.lines.first().map(String::as_str), Some("100"));
    assert_eq!(page.lines.last().map(String::as_str), Some("199"));
  }

  #[test]
  fn paginate_clamps_at_end_of_log() {
    let lines = (0..250).map(|i| i.to_string()).collect::<Vec<_>>();
    let page = paginate(&lines, 200, 100, false);

    assert_eq!(page.start, 200);
    assert_eq!(page.end, 250);
    assert_eq!(page.next_offset, None);
    assert_eq!(page.lines.len(), 50);
    assert_eq!(page.lines.first().map(String::as_str), Some("200"));
    assert_eq!(page.lines.last().map(String::as_str), Some("249"));
  }

  #[test]
  fn paginate_offset_past_end_is_empty() {
    let lines = (0..250).map(|i| i.to_string()).collect::<Vec<_>>();
    let page = paginate(&lines, 500, 100, false);

    assert_eq!(page.start, 250);
    assert_eq!(page.end, 250);
    assert_eq!(page.next_offset, None);
    assert!(page.lines.is_empty());
  }

  #[test]
  fn paginate_from_end_returns_last_lines() {
    let lines = (0..250).map(|i| i.to_string()).collect::<Vec<_>>();
    let page = paginate(&lines, 0, 10, true);

    assert_eq!(page.start, 240);
    assert_eq!(page.end, 250);
    assert_eq!(page.next_offset, Some(10));
    assert_eq!(page.lines.len(), 10);
    assert_eq!(page.lines.first().map(String::as_str), Some("240"));
    assert_eq!(page.lines.last().map(String::as_str), Some("249"));
  }

  #[test]
  fn paginate_from_end_with_offset_walks_backwards() {
    let lines = (0..250).map(|i| i.to_string()).collect::<Vec<_>>();
    let page = paginate(&lines, 100, 10, true);

    assert_eq!(page.start, 140);
    assert_eq!(page.end, 150);
    assert_eq!(page.next_offset, Some(110));
    assert_eq!(page.lines.first().map(String::as_str), Some("140"));
    assert_eq!(page.lines.last().map(String::as_str), Some("149"));
  }

  #[test]
  fn paginate_from_end_offset_past_log_is_empty() {
    let lines = (0..250).map(|i| i.to_string()).collect::<Vec<_>>();
    let page = paginate(&lines, 500, 100, true);

    assert_eq!(page.start, 0);
    assert_eq!(page.end, 0);
    assert_eq!(page.next_offset, None);
    assert!(page.lines.is_empty());
  }

  #[test]
  fn describe_page_middle_page_points_to_next_offset() {
    assert_eq!(
      describe_page(100, 200, 250, Some(200)),
      "[nix_log] lines 100..199 of 250 · next offset: 200"
    );
  }

  #[test]
  fn describe_page_last_page_marks_end_of_log() {
    assert_eq!(
      describe_page(200, 250, 250, None),
      "[nix_log] lines 200..249 of 250 · end of log"
    );
  }

  #[test]
  fn describe_page_from_end_page_points_to_next_offset() {
    assert_eq!(
      describe_page(140, 150, 250, Some(110)),
      "[nix_log] lines 140..149 of 250 · next offset: 110"
    );
  }

  #[test]
  fn describe_page_last_page_from_end_points_backwards() {
    assert_eq!(
      describe_page(240, 250, 250, Some(10)),
      "[nix_log] lines 240..249 of 250 · next offset: 10"
    );
  }

  #[test]
  fn describe_page_from_end_reached_start_marks_end() {
    assert_eq!(
      describe_page(0, 10, 250, None),
      "[nix_log] lines 0..9 of 250 · end of log"
    );
  }

  #[test]
  fn describe_page_offset_past_end_marks_end() {
    assert_eq!(
      describe_page(250, 250, 250, None),
      "[nix_log] end of log (250 total lines)"
    );
  }
}
