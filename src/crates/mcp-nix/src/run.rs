//! Runs the MCP server of the `mcp-nix` binary.

use std::path::{Path, PathBuf};

use anyhow::Context;
use mcp_nix::prelude::*;
use mcp_nix::{bind_root_args, parse_bwrap_command, parse_sandbox_args};
use rmcp::ServiceExt;
use rmcp::transport::stdio;

use crate::config::Config;

/// Serve the MCP protocol over stdio until the client disconnects.
pub async fn run(config: Config) -> anyhow::Result<()> {
  let settings = settings(&config)?;
  settings
    .guard
    .verify()
    .context("the build guard is not going to work")?;
  tracing::info!(
    sandboxed = settings.sandbox.is_some(),
    allowed_commands = settings.allowed_commands.len(),
    allow_fod = config.allow_fod,
    nix_sandboxed = config.nix_bwrap_args.trim() != "",
    "starting the mcp-nix server"
  );

  let service = NixServer::new(settings).serve(stdio()).await?;
  service.waiting().await?;

  tracing::info!("the mcp-nix server stopped");
  Ok(())
}

fn settings(config: &Config) -> anyhow::Result<ServerSettings> {
  let root = root(config)?;

  Ok(ServerSettings {
    guard: guard(config, &root)?,
    sandbox: sandbox(config, &root),
    bwrap_command: parse_bwrap_command(&config.bwrap_command),
    allowed_commands: config.commands.clone(),
    log_limit: config.log_limit,
    root,
  })
}

/// The bubblewrap arguments of the programs: `bwrap_args`, then
/// `bwrap_args_extra`, then the root when the `bind_root` setting says so. The
/// sandbox of `nix` always shows the root read-only and is not a part of this.
fn sandbox(config: &Config, root: &Path) -> Option<Vec<String>> {
  parse_sandbox_args(&config.bwrap_args).map(|mut args| {
    args
      .extend(parse_sandbox_args(&config.bwrap_args_extra).unwrap_or_default());
    if let Some(mode) = config.bind_root {
      args.extend(bind_root_args(root, mode));
    }
    args
  })
}

/// The directory of the tools as a canonical path: the `root` setting, or the
/// working directory of the server when that is blank.
fn root(config: &Config) -> anyhow::Result<PathBuf> {
  let root = match config.root.trim() {
    "" => std::env::current_dir()
      .context("failed to find the working directory of the server")?,
    root => PathBuf::from(root),
  };
  let canonical = root
    .canonicalize()
    .with_context(|| format!("the root {} cannot be used", root.display()))?;
  anyhow::ensure!(
    canonical.is_dir(),
    "the root {} is not a directory",
    canonical.display()
  );

  Ok(canonical)
}

fn guard(config: &Config, root: &Path) -> anyhow::Result<BuildGuard> {
  let guard = if config.allow_fod {
    BuildGuard::unguarded()
  } else {
    let hook = config.pre_build_hook.trim();
    anyhow::ensure!(
      !hook.is_empty(),
      "pre_build_hook is blank, so fixed-output derivations cannot be kept \
       from building: set it to the path of mcp-nix-hook, or set allow_fod"
    );
    BuildGuard::pre_build_hook(hook)
  };

  let guard = guard.in_directory(root.to_string_lossy());

  Ok(match parse_sandbox_args(&config.nix_bwrap_args) {
    Some(mut args) => {
      args.extend(
        parse_sandbox_args(&config.nix_bwrap_args_extra).unwrap_or_default(),
      );
      guard.sandboxed(parse_bwrap_command(&config.nix_bwrap_command), args)
    }
    None => guard,
  })
}

#[cfg(test)]
mod tests {
  use mcp_nix::BindRoot;

  use super::*;

  fn config(sandbox: &str) -> Config {
    Config {
      log_level: "info".to_string(),
      bwrap_command: "nsenter --target 1 bwrap".to_string(),
      bwrap_args: sandbox.to_string(),
      bwrap_args_extra: String::new(),
      add_cuda_bwrap_args: true,
      nix_bwrap_command: "env bwrap".to_string(),
      nix_bwrap_args: "--unshare-net".to_string(),
      nix_bwrap_args_extra: String::new(),
      allow_fod: false,
      pre_build_hook: "/bin/hook".to_string(),
      commands: Vec::new(),
      log_limit: 42,
      root: String::new(),
      bind_root: None,
    }
  }

  fn root_dir() -> PathBuf {
    std::env::temp_dir()
  }

  #[test]
  fn settings_split_the_sandbox_and_the_commands() {
    let config = Config {
      commands: vec!["cargo".to_string(), "make".to_string()],
      ..config("--unshare-all --proc /proc")
    };

    let settings = settings(&config).unwrap();

    assert_eq!(
      settings.sandbox,
      Some(vec![
        "--unshare-all".to_string(),
        "--proc".to_string(),
        "/proc".to_string()
      ])
    );
    assert_eq!(
      settings.allowed_commands,
      vec!["cargo".to_string(), "make".to_string()]
    );
    assert_eq!(
      settings.bwrap_command,
      vec![
        "nsenter".to_string(),
        "--target".to_string(),
        "1".to_string(),
        "bwrap".to_string()
      ]
    );
    assert_eq!(settings.log_limit, 42);
  }

  #[test]
  fn the_root_is_the_working_directory_of_the_server_when_blank() {
    let settings = settings(&config("")).unwrap();

    assert_eq!(
      settings.root,
      std::env::current_dir().unwrap().canonicalize().unwrap()
    );
  }

  #[test]
  fn the_root_setting_is_the_root_and_where_nix_runs() {
    let config = Config {
      root: root_dir().display().to_string(),
      ..config("")
    };
    let canonical = root_dir().canonicalize().unwrap();

    let settings = settings(&config).unwrap();

    assert_eq!(settings.root, canonical);
    assert_eq!(
      settings.guard,
      BuildGuard::pre_build_hook("/bin/hook")
        .in_directory(canonical.to_string_lossy())
        .sandboxed(
          vec!["env".to_string(), "bwrap".to_string()],
          vec!["--unshare-net".to_string()]
        )
    );
  }

  #[test]
  fn the_root_is_bound_after_the_sandbox_arguments_when_asked_to() {
    let canonical = root_dir().canonicalize().unwrap();
    let root = canonical.to_string_lossy().into_owned();

    for (bind_root, flag) in [
      (BindRoot::ReadOnly, "--ro-bind"),
      (BindRoot::ReadWrite, "--bind"),
    ] {
      let config = Config {
        root: root.clone(),
        bind_root: Some(bind_root),
        ..config("--unshare-all --tmpfs /tmp")
      };

      let settings = settings(&config).unwrap();

      assert_eq!(
        settings.sandbox,
        Some(
          ["--unshare-all", "--tmpfs", "/tmp", flag, &root, &root]
            .map(str::to_string)
            .to_vec()
        ),
        "{bind_root:?}"
      );
    }
  }

  #[test]
  fn the_extra_arguments_come_after_the_arguments_and_before_the_root() {
    let canonical = root_dir().canonicalize().unwrap();
    let root = canonical.to_string_lossy().into_owned();
    let config = Config {
      root: root.clone(),
      bwrap_args_extra: "--setenv A b --share-net".to_string(),
      bind_root: Some(BindRoot::ReadOnly),
      ..config("--unshare-all")
    };

    let settings = settings(&config).unwrap();

    assert_eq!(
      settings.sandbox,
      Some(
        [
          "--unshare-all",
          "--setenv",
          "A",
          "b",
          "--share-net",
          "--ro-bind",
          &root,
          &root
        ]
        .map(str::to_string)
        .to_vec()
      )
    );
  }

  #[test]
  fn the_extra_arguments_do_nothing_without_a_sandbox() {
    let config = Config {
      bwrap_args_extra: "--share-net".to_string(),
      nix_bwrap_args: String::new(),
      nix_bwrap_args_extra: "--share-net".to_string(),
      ..config("")
    };

    assert_eq!(settings(&config).unwrap().sandbox, None);
    assert_eq!(
      guard(&config, &root_dir()).unwrap(),
      BuildGuard::pre_build_hook("/bin/hook")
        .in_directory(root_dir().to_string_lossy())
    );
  }

  #[test]
  fn the_nix_extra_arguments_come_after_the_nix_arguments() {
    let config = Config {
      nix_bwrap_args_extra: "--setenv A b".to_string(),
      ..config("")
    };

    assert_eq!(
      guard(&config, &root_dir()).unwrap(),
      BuildGuard::pre_build_hook("/bin/hook")
        .in_directory(root_dir().to_string_lossy())
        .sandboxed(
          vec!["env".to_string(), "bwrap".to_string()],
          ["--unshare-net", "--setenv", "A", "b"]
            .map(str::to_string)
            .to_vec()
        )
    );
  }

  #[test]
  fn the_root_is_left_out_of_the_sandbox_by_default() {
    let settings = settings(&config("--unshare-all")).unwrap();

    assert_eq!(settings.sandbox, Some(vec!["--unshare-all".to_string()]));
  }

  #[test]
  fn binding_the_root_does_nothing_without_a_sandbox() {
    let config = Config {
      bind_root: Some(BindRoot::ReadWrite),
      ..config("")
    };

    assert_eq!(settings(&config).unwrap().sandbox, None);
  }

  #[test]
  fn a_root_that_is_not_a_directory_is_an_error() {
    for root in ["/mcp-nix/no/such/directory", "/etc/passwd"] {
      let config = Config {
        root: root.to_string(),
        ..config("")
      };

      assert!(settings(&config).is_err(), "{root}");
    }
  }

  #[test]
  fn settings_disable_the_sandbox_when_it_is_blank() {
    let settings = settings(&config("")).unwrap();

    assert_eq!(settings.sandbox, None);
    assert!(settings.allowed_commands.is_empty());
  }

  #[test]
  fn the_nix_sandbox_has_a_bwrap_command_of_its_own() {
    assert_eq!(
      guard(&config(""), &root_dir()).unwrap(),
      BuildGuard::pre_build_hook("/bin/hook")
        .in_directory(root_dir().to_string_lossy())
        .sandboxed(
          vec!["env".to_string(), "bwrap".to_string()],
          vec!["--unshare-net".to_string()]
        )
    );
  }

  #[test]
  fn a_blank_nix_sandbox_runs_nix_without_one() {
    let config = Config {
      nix_bwrap_args: " ".to_string(),
      ..config("")
    };

    assert_eq!(
      guard(&config, &root_dir()).unwrap(),
      BuildGuard::pre_build_hook("/bin/hook")
        .in_directory(root_dir().to_string_lossy())
    );
  }

  #[test]
  fn allowing_fixed_output_derivations_needs_no_hook() {
    let config = Config {
      allow_fod: true,
      pre_build_hook: String::new(),
      nix_bwrap_args: String::new(),
      ..config("")
    };

    assert_eq!(
      guard(&config, &root_dir()).unwrap(),
      BuildGuard::unguarded().in_directory(root_dir().to_string_lossy())
    );
  }

  #[test]
  fn a_blank_hook_is_an_error_unless_fixed_output_derivations_are_allowed() {
    let config = Config {
      pre_build_hook: "  ".to_string(),
      ..config("")
    };

    assert!(guard(&config, &root_dir()).is_err());
  }
}
