//! Entry point of the `mcp-nix-hook` binary: a nix `pre-build-hook` that
//! refuses to build fixed-output, impure and sandbox-escaping derivations.
//!
//! Nix runs it before each build it does itself, with the derivation file as
//! the first argument, and does not build when it fails. It runs inside the
//! nix daemon, so what it prints on standard error ends up in the daemon log
//! and not with the caller. It has no arguments or configuration of its own to
//! parse because nix decides how it is called, and it looks `nix` up on the
//! `PATH` of the daemon. It enables `nix-command` for its own `nix` call,
//! because the daemon may not have it enabled.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
  // Nothing to do when logging cannot be set up, and nobody to tell.
  let _ = tracing_subscriber::fmt()
    .json()
    .with_writer(std::io::stderr)
    .try_init();

  let Some(derivation) = std::env::args().nth(1) else {
    tracing::error!("refusing to build: nix gave no derivation");
    return ExitCode::FAILURE;
  };

  match decide(&derivation) {
    Ok(()) => ExitCode::SUCCESS,
    Err(reason) => {
      tracing::error!(
        derivation,
        reason,
        "refusing to build, ask the human to build it first"
      );
      ExitCode::FAILURE
    }
  }
}

/// Whether the derivation file may be built, or why not.
fn decide(derivation: &str) -> Result<(), String> {
  let output = Command::new("nix")
    .args([
      "--extra-experimental-features",
      "nix-command",
      "derivation",
      "show",
      derivation,
    ])
    .output()
    .map_err(|error| format!("failed to run nix: {error}"))?;
  if !output.status.success() {
    return Err(format!(
      "nix derivation show failed: {}",
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }

  mcp_nix::hook::verdict(&output.stdout).map_err(|refusal| refusal.to_string())
}
