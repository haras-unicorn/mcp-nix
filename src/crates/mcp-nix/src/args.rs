//! Command line arguments of the `mcp-nix` binary.

use std::path::PathBuf;

use clap::Parser;

/// MCP server that provides nix tooling.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Args {
  /// The configuration file to read. It must exist. Without it, `mcp-nix.toml`
  /// in the current directory is read when it exists.
  #[arg(long, global = true)]
  pub config: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn config_is_optional() {
    let args = Args::try_parse_from(["mcp-nix"]).unwrap();

    assert_eq!(args.config, None);
  }

  #[test]
  fn config_takes_a_path() {
    let args =
      Args::try_parse_from(["mcp-nix", "--config", "some/mcp-nix.toml"])
        .unwrap();

    assert_eq!(args.config, Some(PathBuf::from("some/mcp-nix.toml")));
  }

  #[test]
  fn unknown_arguments_are_rejected() {
    assert!(Args::try_parse_from(["mcp-nix", "--nope"]).is_err());
  }
}
