//! Sandboxing of programs run from built nix packages.

use std::collections::HashMap;
use std::process::Command;

/// Errors produced while running a program from a built nix package.
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
  /// The program printed nothing to stdout.
  #[error("{program} produced no output")]
  EmptyOutput {
    /// The program being run.
    program: String,
  },
}

/// The default bubblewrap arguments, used when `MCP_NIX_SANDBOX` is unset.
///
/// Network is restricted: the package is built with `nix build` outside the
/// sandbox, and the program run inside the sandbox is not expected to need the
/// network.
pub const DEFAULT_SANDBOX_ARGS: &str = "\
--die-with-parent --unshare-user --unshare-ipc --unshare-pid --unshare-net \
--ro-bind /nix /nix --ro-bind /etc /etc \
--ro-bind /usr /usr --ro-bind /bin /bin --ro-bind /lib /lib --ro-bind /lib64 /lib64 \
--tmpfs /tmp --tmpfs /home --tmpfs /run --proc /proc --dev /dev";

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

/// The minimal environment always set for programs run from built packages.
///
/// The full environment is cleared before running a program, so that it does
/// not inherit the environment of the MCP server. Only these variables are set
/// by default; additional variables can be provided per invocation.
pub const BASE_ENV: &[(&str, &str)] = &[
  ("HOME", "/tmp"),
  ("LANG", "C.UTF-8"),
  ("PATH", "/usr/bin:/bin:/nix/var/nix/profiles/default/bin"),
  ("TMPDIR", "/tmp"),
];

/// Options for running a program from a built nix package.
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
}

/// Resolve `bwrap` to an absolute path using the current process's `PATH`.
///
/// The program runs with a cleared environment, so `bwrap` must be located
/// before its own `PATH` is cleared.
fn resolve_bwrap() -> Result<String, SandboxError> {
  let path =
    std::env::var_os("PATH").ok_or_else(|| SandboxError::SandboxSpawn {
      source: std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "PATH is not set",
      ),
    })?;

  for dir in std::env::split_paths(&path) {
    let candidate = dir.join("bwrap");
    if is_executable(&candidate) {
      return Ok(candidate.to_string_lossy().into_owned());
    }
  }

  Err(SandboxError::SandboxSpawn {
    source: std::io::Error::new(
      std::io::ErrorKind::NotFound,
      "bwrap not found on PATH",
    ),
  })
}

fn is_executable(path: &std::path::Path) -> bool {
  use std::os::unix::fs::PermissionsExt;
  path.metadata().is_ok_and(|metadata| {
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
  })
}

/// Run a program from a built nix package, optionally wrapped in a bubblewrap
/// sandbox, and return its standard output.
///
/// The program runs with a cleared environment containing only [`BASE_ENV`]
/// plus the variables in [`RunOptions::env`].
pub fn run_program(
  store_path: &str,
  program: &str,
  options: &RunOptions,
) -> Result<String, SandboxError> {
  let bin = format!("{store_path}/bin/{program}");
  let sandboxed = options.sandbox.is_some();

  let mut command = match &options.sandbox {
    Some(bwrap_args) => {
      let bwrap = resolve_bwrap()?;
      let mut args: Vec<String> = bwrap_args.to_vec();
      if let Some(cwd) = &options.cwd {
        args.push("--chdir".to_string());
        args.push(cwd.clone());
      }
      args.push(bin.clone());
      args.extend(options.args.iter().cloned());
      let mut command = Command::new(bwrap);
      command.args(args);
      command
    }
    None => {
      let mut command = Command::new(&bin);
      if let Some(cwd) = &options.cwd {
        command.current_dir(cwd);
      }
      command.args(&options.args);
      command
    }
  };

  command.env_clear();
  command.envs(BASE_ENV.iter().copied());
  for (key, value) in &options.env {
    command.env(key, value);
  }

  let output = command.output().map_err(|source| {
    if sandboxed {
      SandboxError::SandboxSpawn { source }
    } else {
      SandboxError::Spawn {
        program: bin.clone(),
        source,
      }
    }
  })?;

  if !output.status.success() {
    return Err(SandboxError::Failed {
      program: bin,
      stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    });
  }

  let stdout = String::from_utf8_lossy(&output.stdout);
  let stdout = stdout.trim();
  if stdout.is_empty() {
    return Err(SandboxError::EmptyOutput { program: bin });
  }
  Ok(stdout.to_string())
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
}
