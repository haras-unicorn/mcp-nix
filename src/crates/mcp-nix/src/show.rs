//! The commands that print something and exit: the schema of the
//! configuration and the effective configuration.

use std::io::Write;
use std::path::Path;

use anyhow::Context;
use rmcp::schemars;

use crate::args::{Command, Format};
use crate::config::Config;

/// Run a command and print what it shows to standard output.
pub fn show(
  command: &Command,
  config_file: Option<&Path>,
) -> anyhow::Result<()> {
  let text = match command {
    Command::Schema => schema()?,
    Command::Config { format } => config(&Config::load(config_file)?, *format)?,
  };

  let mut stdout = std::io::stdout().lock();
  writeln!(stdout, "{}", text.trim_end())
    .context("failed to write to standard output")
}

/// The JSON schema of the configuration.
pub fn schema() -> anyhow::Result<String> {
  serde_json::to_string_pretty(&schemars::schema_for!(Config))
    .context("failed to render the schema")
}

/// The configuration in the given format.
pub fn config(config: &Config, format: Format) -> anyhow::Result<String> {
  match format {
    Format::Toml => toml::to_string_pretty(config)
      .context("failed to render the configuration as TOML"),
    Format::Yaml => serde_yaml_ng::to_string(config)
      .context("failed to render the configuration as YAML"),
    Format::Json => serde_json::to_string_pretty(config)
      .context("failed to render the configuration as JSON"),
  }
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::path::PathBuf;

  use super::*;

  /// A configuration file with the given contents that is removed on drop.
  struct TempFile {
    path: PathBuf,
  }

  impl TempFile {
    fn new(name: &str, contents: &str) -> Self {
      let path = std::env::temp_dir()
        .join(format!("mcp-nix-show-{}-{name}.toml", std::process::id()));
      fs::write(&path, contents).unwrap();
      Self { path }
    }
  }

  impl Drop for TempFile {
    fn drop(&mut self) {
      let _ = fs::remove_file(&self.path);
    }
  }

  fn loaded(name: &str, contents: &str) -> Config {
    let file = TempFile::new(name, contents);

    Config::load(Some(&file.path)).unwrap()
  }

  #[test]
  fn the_schema_describes_every_setting_and_nothing_else() {
    let schema: serde_json::Value =
      serde_json::from_str(&schema().unwrap()).unwrap();

    let properties = schema["properties"].as_object().unwrap();
    for key in ["bwrap_args", "add_cuda_bwrap_args", "bind_root", "commands"] {
      assert!(properties.contains_key(key), "{key}");
    }
    assert!(properties["bwrap_args"]["description"].is_string());
    assert_eq!(schema["additionalProperties"], false);
  }

  #[test]
  fn the_configuration_is_printed_in_a_format_that_reads_back() {
    let config = loaded(
      "round-trip",
      "log_limit = 7\nbind_root = \"read-only\"\ncommands = [\"cargo\"]\n",
    );

    let toml = super::config(&config, Format::Toml).unwrap();
    let yaml = super::config(&config, Format::Yaml).unwrap();
    let json = super::config(&config, Format::Json).unwrap();

    assert_eq!(toml::from_str::<Config>(&toml).unwrap(), config);
    assert_eq!(serde_yaml_ng::from_str::<Config>(&yaml).unwrap(), config);
    assert_eq!(serde_json::from_str::<Config>(&json).unwrap(), config);
  }

  #[test]
  fn the_configuration_shows_what_was_set_and_what_was_not() {
    let config = loaded("values", "log_limit = 7\n");

    let json: serde_json::Value =
      serde_json::from_str(&super::config(&config, Format::Json).unwrap())
        .unwrap();

    assert_eq!(json["log_limit"], 7);
    assert_eq!(json["log_level"], "info");
    assert_eq!(json["add_cuda_bwrap_args"], true);
  }

  #[test]
  fn a_setting_that_is_not_set_is_left_out() {
    let config = loaded("unset", "");

    let toml = super::config(&config, Format::Toml).unwrap();

    assert!(!toml.contains("bind_root"), "{toml}");
  }
}
