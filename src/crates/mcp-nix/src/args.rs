//! Command line arguments of the `mcp-nix` binary.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// MCP server that provides nix tooling.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Args {
  /// The configuration file to read. It must exist. Without it, `mcp-nix.toml`
  /// in the current directory is read when it exists.
  #[arg(long, global = true)]
  pub config: Option<PathBuf>,
  /// What to do instead of serving over stdio.
  #[command(subcommand)]
  pub command: Option<Command>,
}

/// The commands that print something and exit, in place of the server.
#[derive(Debug, Subcommand)]
pub enum Command {
  /// Print the JSON schema of the configuration.
  Schema,
  /// Print the effective configuration: the defaults, the configuration file
  /// and the environment together, with the arguments for the GPU of the
  /// machine in `bwrap_args` when the server would add them.
  Config {
    /// The format to print the configuration in.
    #[arg(long, value_enum, default_value_t = Format::Toml)]
    format: Format,
  },
}

/// A format the `config` command prints the configuration in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
  /// TOML, like the configuration file.
  Toml,
  /// YAML.
  Yaml,
  /// JSON.
  Json,
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

  #[test]
  fn without_a_command_the_server_is_served() {
    let args = Args::try_parse_from(["mcp-nix"]).unwrap();

    assert!(args.command.is_none());
  }

  #[test]
  fn the_schema_command_takes_no_arguments() {
    let args = Args::try_parse_from(["mcp-nix", "schema"]).unwrap();

    assert!(matches!(args.command, Some(Command::Schema)));
    assert!(
      Args::try_parse_from(["mcp-nix", "schema", "--format", "json"]).is_err()
    );
  }

  #[test]
  fn the_config_command_prints_toml_by_default() {
    let args = Args::try_parse_from(["mcp-nix", "config"]).unwrap();

    assert!(matches!(
      args.command,
      Some(Command::Config {
        format: Format::Toml
      })
    ));
  }

  #[test]
  fn the_config_command_takes_a_format() {
    for (name, expected) in [
      ("toml", Format::Toml),
      ("yaml", Format::Yaml),
      ("json", Format::Json),
    ] {
      let args =
        Args::try_parse_from(["mcp-nix", "config", "--format", name]).unwrap();

      assert!(
        matches!(args.command, Some(Command::Config { format }) if format == expected),
        "{name}"
      );
    }
  }

  #[test]
  fn an_unknown_format_is_rejected() {
    assert!(
      Args::try_parse_from(["mcp-nix", "config", "--format", "ini"]).is_err()
    );
  }

  #[test]
  fn the_config_file_can_come_before_or_after_a_command() {
    for argv in [
      ["mcp-nix", "--config", "a.toml", "config"],
      ["mcp-nix", "config", "--config", "a.toml"],
    ] {
      let args = Args::try_parse_from(argv).unwrap();

      assert_eq!(args.config, Some(PathBuf::from("a.toml")), "{argv:?}");
    }
  }
}
