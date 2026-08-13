mod common;

use std::collections::HashMap;
use std::fs;

use common::{
  TEST_BWRAP_ARGS, TempFlake, bwrap_usable, flake_nix, for_each_flake,
  nix_store_dir, store_bin_dir,
};
use mcp_nix::RunOptions;

#[test]
fn develop_runs_command_and_returns_output() {
  let Some(store_dir) = nix_store_dir() else {
    eprintln!("skipping develop test: nix is not in the nix store");
    return;
  };
  for_each_flake(|fixture| {
    let command = format!("{store_dir}/bin/nix");
    let options = RunOptions {
      args: vec!["--version".to_string()],
      ..Default::default()
    };
    let output = mcp_nix::develop(
      &fixture.flake.dir.display().to_string(),
      &command,
      &options,
    )
    .unwrap();

    assert!(output.contains("nix"), "unexpected output: {output}");
  });
}

#[test]
fn develop_sets_clean_environment_when_sandboxed() {
  if !bwrap_usable() {
    return;
  }
  let Some(store_dir) = store_bin_dir("env") else {
    eprintln!("skipping environment test: env is not in the nix store");
    return;
  };
  for_each_flake(|fixture| {
    let sandbox: Vec<String> =
      TEST_BWRAP_ARGS.iter().map(|arg| arg.to_string()).collect();
    let command = format!("{store_dir}/bin/env");
    let options = RunOptions {
      env: HashMap::from([(
        "MCP_NIX_TEST_VAR".to_string(),
        "hello-from-env".to_string(),
      )]),
      sandbox: Some(sandbox),
      ..Default::default()
    };
    let output = mcp_nix::develop(
      &fixture.flake.dir.display().to_string(),
      &command,
      &options,
    )
    .unwrap();

    assert!(
      output.contains("MCP_NIX_TEST_VAR=hello-from-env"),
      "custom env missing: {output}"
    );
    assert!(
      output.contains("MCP_NIX_DEVELOP_TEST=from-dev-shell"),
      "dev shell env missing: {output}"
    );
    assert!(
      output.contains("MCP_NIX_SHELL_HOOK=ran"),
      "shell hook did not run: {output}"
    );
  });
}

#[test]
fn develop_sets_working_directory() {
  let Some(store_dir) = store_bin_dir("pwd") else {
    eprintln!("skipping cwd test: pwd is not in the nix store");
    return;
  };
  for_each_flake(|fixture| {
    let dir = std::env::temp_dir()
      .join(format!("mcp-nix-develop-cwd-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let expected = fs::canonicalize(&dir)
      .unwrap()
      .to_string_lossy()
      .into_owned();

    let command = format!("{store_dir}/bin/pwd");
    let options = RunOptions {
      cwd: Some(dir.to_string_lossy().into_owned()),
      ..Default::default()
    };
    let output = mcp_nix::develop(
      &fixture.flake.dir.display().to_string(),
      &command,
      &options,
    )
    .unwrap();

    assert_eq!(output, expected);
    let _ = fs::remove_dir_all(&dir);
  });
}

#[test]
fn develop_runs_under_bwrap_when_sandboxed() {
  if !bwrap_usable() {
    return;
  }
  let Some(store_dir) = nix_store_dir() else {
    eprintln!("skipping bwrap develop test: nix is not in the nix store");
    return;
  };
  for_each_flake(|fixture| {
    let sandbox: Vec<String> =
      TEST_BWRAP_ARGS.iter().map(|arg| arg.to_string()).collect();
    let command = format!("{store_dir}/bin/nix");
    let options = RunOptions {
      args: vec!["--version".to_string()],
      sandbox: Some(sandbox),
      ..Default::default()
    };
    let output = mcp_nix::develop(
      &fixture.flake.dir.display().to_string(),
      &command,
      &options,
    )
    .unwrap();

    assert!(output.contains("nix"), "unexpected output: {output}");
  });
}

#[test]
fn develop_sandboxed_command_has_no_network() {
  if !bwrap_usable() {
    return;
  }
  let Some(bash) = store_bin_dir("bash") else {
    eprintln!("skipping network test: bash is not in the nix store");
    return;
  };
  for_each_flake(|fixture| {
    let mut sandbox: Vec<String> =
      TEST_BWRAP_ARGS.iter().map(|arg| arg.to_string()).collect();
    sandbox.push("--unshare-net".to_string());
    let command = format!("{bash}/bin/bash");
    let options = RunOptions {
      args: vec!["-c".to_string(), "exec 3<>/dev/tcp/8.8.8.8/53".to_string()],
      sandbox: Some(sandbox),
      ..Default::default()
    };
    let error = mcp_nix::develop(
      &fixture.flake.dir.display().to_string(),
      &command,
      &options,
    )
    .unwrap_err();

    assert!(
      error.to_string().contains("command"),
      "unexpected error: {error}"
    );
  });
}

#[test]
fn develop_rejects_command_not_in_allowed_commands() {
  let options = RunOptions {
    allowed_commands: vec!["some-other-command".to_string()],
    ..Default::default()
  };
  let error =
    mcp_nix::develop("path:./does-not-matter", "cargo", &options).unwrap_err();

  assert!(
    error.to_string().contains("MCP_NIX_COMMANDS"),
    "unexpected error: {error}"
  );
}

#[test]
fn develop_runs_command_in_allowed_commands() {
  if !bwrap_usable() {
    return;
  }
  let Some(store_dir) = nix_store_dir() else {
    eprintln!("skipping allow list test: nix is not in the nix store");
    return;
  };
  for_each_flake(|fixture| {
    let sandbox: Vec<String> =
      TEST_BWRAP_ARGS.iter().map(|arg| arg.to_string()).collect();
    let command = format!("{store_dir}/bin/nix");
    let options = RunOptions {
      args: vec!["--version".to_string()],
      allowed_commands: vec!["nix".to_string()],
      sandbox: Some(sandbox),
      ..Default::default()
    };
    let output = mcp_nix::develop(
      &fixture.flake.dir.display().to_string(),
      &command,
      &options,
    )
    .unwrap();

    assert!(output.contains("nix"), "unexpected output: {output}");
  });
}

#[test]
fn develop_returns_error_for_failing_command() {
  for_each_flake(|fixture| {
    let command = "/bin/sh".to_string();
    let options = RunOptions {
      args: vec!["-c".to_string(), "echo boom >&2; exit 1".to_string()],
      ..Default::default()
    };
    let error = mcp_nix::develop(
      &fixture.flake.dir.display().to_string(),
      &command,
      &options,
    )
    .unwrap_err();

    assert!(
      error.to_string().contains("command /bin/sh failed"),
      "unexpected error: {error}"
    );
    assert!(
      error.to_string().contains("boom"),
      "stderr not reported: {error}"
    );
  });
}

#[test]
fn develop_returns_error_for_missing_flake() {
  if store_bin_dir("bash").is_none() {
    eprintln!("skipping missing flake test: bash is not in the nix store");
    return;
  }
  let flake =
    TempFlake::new(&flake_nix("echo mcp-nix-develop-test > \"$out\""));

  let command = "/bin/sh".to_string();
  let options = RunOptions::default();
  let missing = flake.dir.join("does-not-exist.flake");
  let error =
    mcp_nix::develop(&missing.display().to_string(), &command, &options)
      .unwrap_err();

  assert!(
    error
      .to_string()
      .contains("failed to capture the dev shell environment"),
    "unexpected error: {error}"
  );
}
