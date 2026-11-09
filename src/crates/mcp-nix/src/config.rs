//! Configuration of the `mcp-nix` binary.
//!
//! Values come from, in increasing priority, the defaults here, a TOML file and
//! the environment as `MCP_NIX__<KEY>`, for example `MCP_NIX__BWRAP_ARGS`.

use std::path::Path;

// `::config` is the crate and not this module of the same name.
use ::config::{Environment, File};
use anyhow::Context;
use mcp_nix::{
  BindRoot, DEFAULT_BWRAP_COMMAND, DEFAULT_NIX_SANDBOX_ARGS,
  DEFAULT_SANDBOX_ARGS, cuda_sandbox_args,
};
use rmcp::schemars;

/// The name, without extension, of the configuration file read from the
/// current directory when no file is given. It is a const because it has to be
/// known before any configuration can be read.
const DEFAULT_FILE: &str = "mcp-nix";

/// The prefix of the environment variables that configure the program. It is a
/// const because it has to be known before any configuration can be read.
const ENVIRONMENT_PREFIX: &str = "MCP_NIX";

/// The separator of the environment variable prefix and of nested keys.
const ENVIRONMENT_SEPARATOR: &str = "__";

/// The separator of the entries of a list in an environment variable.
const ENVIRONMENT_LIST_SEPARATOR: &str = ",";

/// The keys that are lists, and so are split on the separator when they come
/// from the environment. The other keys are not, because their values can hold
/// the separator.
const LIST_KEYS: [&str; 1] = ["commands"];

/// Settings of the `mcp-nix` binary.
#[derive(
  Debug,
  Clone,
  PartialEq,
  Eq,
  serde::Serialize,
  serde::Deserialize,
  schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Config {
  /// The lowest level of the events that are logged: `off`, `error`, `warn`,
  /// `info`, `debug` or `trace`.
  pub log_level: String,
  /// The command, split on whitespace, that runs bubblewrap for the programs
  /// `nix_run` and `nix_develop` run, for example
  /// `nsenter --target 1 --mount bwrap`. Only its first word is looked up on
  /// the `PATH`.
  pub bwrap_command: String,
  /// Bubblewrap arguments, split on whitespace, that wrap the programs
  /// `nix_run` and `nix_develop` run. Blank disables the sandbox.
  pub bwrap_args: String,
  /// Bubblewrap arguments, split on whitespace, added after `bwrap_args`, so
  /// that the default can be kept and built on. It has no effect when
  /// `bwrap_args` is blank.
  pub bwrap_args_extra: String,
  /// Add the CUDA arguments of the NVIDIA GPUs of the machine after
  /// `bwrap_args`, when there are any. It is on by default and applies to any
  /// `bwrap_args`, so a replaced list keeps the GPU unless this is turned off.
  /// It has no effect when `bwrap_args` is blank. The sandbox of `nix` never
  /// gets them, since nothing there uses a GPU.
  pub add_cuda_bwrap_args: bool,
  /// The command that runs bubblewrap for `nix`, like `bwrap_command`. It is a
  /// setting of its own because the two sandboxes can need different ways in,
  /// for example when only the one of the programs runs in a container.
  pub nix_bwrap_command: String,
  /// Bubblewrap arguments, split on whitespace, that wrap the `nix` commands
  /// of the tools, which keeps them off the network. The working directory is
  /// added read-only. Blank disables the sandbox. Binding the socket of the
  /// nix daemon is what lets `nix` build, so do not bind it into `bwrap_args`.
  pub nix_bwrap_args: String,
  /// Bubblewrap arguments, split on whitespace, added after `nix_bwrap_args`,
  /// like `bwrap_args_extra`. It has no effect when `nix_bwrap_args` is blank.
  pub nix_bwrap_args_extra: String,
  /// Let `nix` build fixed-output derivations, which have network access,
  /// and the other derivations the pre-build hook refuses. When it is false
  /// the server refuses to start without a working `pre_build_hook`.
  pub allow_fod: bool,
  /// The absolute path of the program that nix runs before each build, which
  /// refuses the derivations that must not be built. The `mcp-nix-hook`
  /// binary is that program. It is only used when `allow_fod` is false.
  pub pre_build_hook: String,
  /// The command file names `nix_run` and `nix_develop` may execute. An empty
  /// list allows any command. In the environment the names are separated by
  /// commas, without spaces around them.
  pub commands: Vec<String>,
  /// The number of lines per `nix_log` page when the caller gives no limit.
  pub log_limit: usize,
  /// The directory of the tools: where the `nix` commands run, what the
  /// working directory of `nix_run` and `nix_develop` has to be in, and what
  /// a relative one is taken from. Blank is the working directory of the
  /// server.
  pub root: String,
  /// Bind the root into the sandbox of the programs, after `bwrap_args` and
  /// `bwrap_args_extra`: `read-only` or `read-write`. Leaving it out is the
  /// default, since the programs are then able to see, and with `read-write`
  /// change, what is in it. It has no effect when `bwrap_args` is blank.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub bind_root: Option<BindRoot>,
}

impl Config {
  /// Load the effective configuration: the defaults, the given file, which
  /// must exist, or `mcp-nix.toml` in the current directory, which may be
  /// missing, and the environment, with the CUDA arguments of the GPUs of the
  /// machine added to `bwrap_args` when [`Config::with_cuda`] says so.
  pub fn load(file: Option<&Path>) -> anyhow::Result<Self> {
    let config = Self::build(file, environment())?;

    Ok(config.with_cuda(cuda_sandbox_args().as_deref()))
  }

  /// This configuration with the given CUDA arguments after `bwrap_args`, or
  /// as it is when there are none because there is no GPU, when
  /// `add_cuda_bwrap_args` is off, or when `bwrap_args` is blank, which means
  /// no sandbox.
  pub fn with_cuda(self, cuda_args: Option<&str>) -> Self {
    match cuda_args {
      Some(cuda_args)
        if self.add_cuda_bwrap_args && !self.bwrap_args.trim().is_empty() =>
      {
        Self {
          bwrap_args: format!("{} {cuda_args}", self.bwrap_args),
          ..self
        }
      }
      _ => self,
    }
  }

  fn build(
    file: Option<&Path>,
    environment: Environment,
  ) -> anyhow::Result<Self> {
    let builder = ::config::Config::builder()
      .set_default("log_level", "info")?
      .set_default("bwrap_command", DEFAULT_BWRAP_COMMAND)?
      .set_default("bwrap_args", DEFAULT_SANDBOX_ARGS)?
      .set_default("bwrap_args_extra", "")?
      .set_default("add_cuda_bwrap_args", true)?
      .set_default("nix_bwrap_command", DEFAULT_BWRAP_COMMAND)?
      .set_default("nix_bwrap_args", DEFAULT_NIX_SANDBOX_ARGS)?
      .set_default("nix_bwrap_args_extra", "")?
      .set_default("allow_fod", false)?
      .set_default("pre_build_hook", "")?
      .set_default("commands", Vec::<String>::new())?
      .set_default("log_limit", 100_i64)?
      .set_default("root", "")?;

    let builder = match file {
      Some(path) => builder.add_source(File::from(path).required(true)),
      None => builder.add_source(File::with_name(DEFAULT_FILE).required(false)),
    };

    builder
      .add_source(environment)
      .build()
      .context("failed to read the configuration")?
      .try_deserialize()
      .context("invalid configuration")
  }
}

fn environment() -> Environment {
  let environment = Environment::with_prefix(ENVIRONMENT_PREFIX)
    .prefix_separator(ENVIRONMENT_SEPARATOR)
    .separator(ENVIRONMENT_SEPARATOR)
    .list_separator(ENVIRONMENT_LIST_SEPARATOR)
    .try_parsing(true);

  LIST_KEYS.into_iter().fold(environment, |environment, key| {
    environment.with_list_parse_key(key)
  })
}

#[cfg(test)]
mod tests {
  use std::collections::HashMap;
  use std::fs;
  use std::path::PathBuf;

  use mcp_nix::DEFAULT_CUDA_SANDBOX_ARGS;

  use super::*;

  /// A configuration file with the given contents that is removed on drop.
  struct TempFile {
    path: PathBuf,
  }

  impl TempFile {
    fn new(name: &str, contents: &str) -> Self {
      let path = std::env::temp_dir()
        .join(format!("mcp-nix-config-{}-{name}.toml", std::process::id()));
      fs::write(&path, contents).unwrap();
      Self { path }
    }
  }

  impl Drop for TempFile {
    fn drop(&mut self) {
      let _ = fs::remove_file(&self.path);
    }
  }

  fn env(pairs: &[(&str, &str)]) -> Environment {
    let source = pairs
      .iter()
      .map(|(key, value)| (key.to_string(), value.to_string()))
      .collect::<HashMap<_, _>>();
    environment().source(Some(source))
  }

  #[test]
  fn defaults_apply_without_a_file_or_environment() {
    let file = TempFile::new("defaults", "");

    let config = Config::build(Some(&file.path), env(&[])).unwrap();

    assert_eq!(config.log_level, "info");
    assert_eq!(config.bwrap_command, DEFAULT_BWRAP_COMMAND);
    assert_eq!(config.bwrap_args, DEFAULT_SANDBOX_ARGS);
    assert_eq!(config.bwrap_args_extra, "");
    assert!(config.add_cuda_bwrap_args);
    assert_eq!(config.nix_bwrap_command, DEFAULT_BWRAP_COMMAND);
    assert_eq!(config.nix_bwrap_args, DEFAULT_NIX_SANDBOX_ARGS);
    assert_eq!(config.nix_bwrap_args_extra, "");
    assert!(!config.allow_fod);
    assert_eq!(config.pre_build_hook, "");
    assert!(config.commands.is_empty());
    assert_eq!(config.log_limit, 100);
    assert_eq!(config.root, "");
    assert_eq!(config.bind_root, None);
  }

  #[test]
  fn any_sandbox_gets_the_cuda_arguments_and_the_nix_one_does_not() {
    for value in [DEFAULT_SANDBOX_ARGS, "--unshare-all"] {
      let file = TempFile::new("cuda", &format!("bwrap_args = \"{value}\"\n"));
      let config = Config::build(Some(&file.path), env(&[])).unwrap();

      let config = config.with_cuda(Some("--cuda"));

      assert_eq!(config.bwrap_args, format!("{value} --cuda"));
      assert_eq!(config.nix_bwrap_args, DEFAULT_NIX_SANDBOX_ARGS);
    }
  }

  #[test]
  fn the_sandbox_is_left_alone_without_a_gpu() {
    let file = TempFile::new("no-gpu", "");
    let config = Config::build(Some(&file.path), env(&[])).unwrap();

    assert_eq!(config.clone().with_cuda(None), config);
  }

  #[test]
  fn the_sandbox_is_left_alone_when_the_setting_is_off() {
    let file = TempFile::new("off", "add_cuda_bwrap_args = false\n");
    let config = Config::build(Some(&file.path), env(&[])).unwrap();

    let config = config.with_cuda(Some("--cuda"));

    assert_eq!(config.bwrap_args, DEFAULT_SANDBOX_ARGS);
  }

  #[test]
  fn a_blank_sandbox_stays_blank_with_a_gpu() {
    let file = TempFile::new("blank", "bwrap_args = \" \"\n");
    let config = Config::build(Some(&file.path), env(&[])).unwrap();

    let config = config.with_cuda(Some("--cuda"));

    assert_eq!(config.bwrap_args, " ");
  }

  #[test]
  fn the_extra_arguments_stay_after_the_cuda_arguments() {
    let file = TempFile::new("extra", "bwrap_args_extra = \"--share-net\"\n");
    let config = Config::build(Some(&file.path), env(&[])).unwrap();

    let config = config.with_cuda(Some(DEFAULT_CUDA_SANDBOX_ARGS));

    assert_eq!(config.bwrap_args_extra, "--share-net");
    assert!(config.bwrap_args.ends_with(DEFAULT_CUDA_SANDBOX_ARGS));
  }

  #[test]
  fn file_overrides_the_defaults() {
    let file = TempFile::new(
      "file",
      "commands = [\"cargo\", \"make\"]\nlog_limit = 5\nbwrap_args = \"\"\n\
bind_root = \"read-write\"\n",
    );

    let config = Config::build(Some(&file.path), env(&[])).unwrap();

    assert_eq!(config.commands, ["cargo", "make"]);
    assert_eq!(config.log_limit, 5);
    assert_eq!(config.bwrap_args, "");
    assert_eq!(config.bind_root, Some(BindRoot::ReadWrite));
  }

  #[test]
  fn environment_overrides_the_file() {
    let file = TempFile::new("environment", "log_limit = 5\n");

    let config = Config::build(
      Some(&file.path),
      env(&[
        ("MCP_NIX__LOG_LIMIT", "9"),
        ("MCP_NIX__BWRAP_ARGS", "--unshare-all"),
        ("MCP_NIX__BWRAP_ARGS_EXTRA", "--share-net"),
        ("MCP_NIX__ADD_CUDA_BWRAP_ARGS", "false"),
        ("MCP_NIX__NIX_BWRAP_ARGS", "--unshare-net"),
        ("MCP_NIX__NIX_BWRAP_ARGS_EXTRA", "--setenv A b"),
        ("MCP_NIX__BWRAP_COMMAND", "nsenter bwrap"),
        ("MCP_NIX__NIX_BWRAP_COMMAND", "env bwrap"),
        ("MCP_NIX__ALLOW_FOD", "true"),
        ("MCP_NIX__PRE_BUILD_HOOK", "/bin/hook"),
        ("MCP_NIX__ROOT", "/work"),
        ("MCP_NIX__BIND_ROOT", "read-only"),
        ("MCP_NIX__COMMANDS", "cargo,make"),
      ]),
    )
    .unwrap();

    assert!(config.allow_fod);
    assert_eq!(config.pre_build_hook, "/bin/hook");
    assert_eq!(config.log_limit, 9);
    assert_eq!(config.bwrap_args, "--unshare-all");
    assert_eq!(config.bwrap_args_extra, "--share-net");
    assert!(!config.add_cuda_bwrap_args);
    assert_eq!(config.nix_bwrap_args, "--unshare-net");
    assert_eq!(config.nix_bwrap_args_extra, "--setenv A b");
    assert_eq!(config.bwrap_command, "nsenter bwrap");
    assert_eq!(config.nix_bwrap_command, "env bwrap");
    assert_eq!(config.root, "/work");
    assert_eq!(config.bind_root, Some(BindRoot::ReadOnly));
    assert_eq!(config.commands, ["cargo", "make"]);
  }

  #[test]
  fn one_command_in_the_environment_is_a_list_of_one() {
    let file = TempFile::new("one-command", "");

    let config =
      Config::build(Some(&file.path), env(&[("MCP_NIX__COMMANDS", "cargo")]))
        .unwrap();

    assert_eq!(config.commands, ["cargo"]);
  }

  #[test]
  fn an_empty_commands_variable_allows_nothing() {
    let file = TempFile::new("empty-commands", "");

    let config =
      Config::build(Some(&file.path), env(&[("MCP_NIX__COMMANDS", "")]))
        .unwrap();

    assert_eq!(config.commands, [""]);
  }

  #[test]
  fn a_lone_boolean_in_the_environment_is_not_a_list_of_commands() {
    let file = TempFile::new("boolean-command", "");

    let result =
      Config::build(Some(&file.path), env(&[("MCP_NIX__COMMANDS", "true")]));

    assert!(result.is_err());
  }

  #[test]
  fn the_environment_only_splits_the_lists() {
    let file = TempFile::new("split", "");

    let config = Config::build(
      Some(&file.path),
      env(&[("MCP_NIX__BWRAP_ARGS", "--setenv A b,c")]),
    )
    .unwrap();

    assert_eq!(config.bwrap_args, "--setenv A b,c");
  }

  #[test]
  fn an_unknown_bind_root_is_an_error() {
    let file = TempFile::new("bind-root", "");

    for value in ["true", "rw", "read_write", "Read-Only", ""] {
      let result =
        Config::build(Some(&file.path), env(&[("MCP_NIX__BIND_ROOT", value)]));

      assert!(result.is_err(), "{value:?}");
    }
  }

  #[test]
  fn a_missing_file_is_an_error() {
    let missing = std::env::temp_dir().join("mcp-nix-config-missing.toml");

    assert!(Config::build(Some(&missing), env(&[])).is_err());
  }

  #[test]
  fn unknown_keys_are_an_error() {
    let file = TempFile::new("unknown", "nope = 1\n");

    assert!(Config::build(Some(&file.path), env(&[])).is_err());
  }
}
