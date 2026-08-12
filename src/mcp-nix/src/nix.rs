//! Wrappers around `nix` commands.

use crate::sandbox;
use crate::sandbox::SandboxError;
use std::process::Command;

/// Errors produced while building a nix package.
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
  /// Failed to run a program from the built package.
  #[error("failed to run the built package: {0}")]
  Run(#[source] SandboxError),
}

/// Build the given nix package and return its nix store path.
pub fn build_package(package: &str) -> Result<String, NixError> {
  let output = Command::new("nix")
    .args(["build", "--no-link", "--print-out-paths", package])
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

/// Build the given nix package and run one of its programs, returning the
/// program's standard output.
pub fn run_package(
  package: &str,
  program: &str,
  options: &sandbox::RunOptions,
) -> Result<String, NixError> {
  let store_path = build_package(package)?;
  sandbox::run_program(&store_path, program, options).map_err(NixError::Run)
}
