mod common;

use std::collections::HashMap;
use std::fs;

use common::{
  TEST_BWRAP_ARGS, bwrap_usable, for_each_flake, nix_store_dir, store_bin_dir,
};
use mcp_nix::RunOptions;

#[test]
fn run_program_returns_program_output() {
  let Some(store_dir) = nix_store_dir() else {
    eprintln!("skipping run_program test: nix is not in the nix store");
    return;
  };

  let options = RunOptions {
    args: vec!["--version".to_string()],
    ..Default::default()
  };
  let output = mcp_nix::run_program(&store_dir, "nix", &options).unwrap();

  assert!(output.contains("nix"), "unexpected output: {output}");
}

#[test]
fn run_program_inherits_environment_when_unsandboxed() {
  let Some(store_dir) = store_bin_dir("env") else {
    eprintln!("skipping environment test: env is not in the nix store");
    return;
  };

  let options = RunOptions {
    env: HashMap::from([(
      "MCP_NIX_TEST_VAR".to_string(),
      "hello-from-env".to_string(),
    )]),
    ..Default::default()
  };
  let output = mcp_nix::run_program(&store_dir, "env", &options).unwrap();

  assert!(
    output.contains("MCP_NIX_TEST_VAR=hello-from-env"),
    "custom env missing: {output}"
  );
  let path = std::env::var("PATH").unwrap_or_default();
  assert!(
    output.contains(&format!("PATH={path}")),
    "server PATH not inherited: {output}"
  );
}

#[test]
fn run_program_sets_working_directory() {
  let Some(store_dir) = store_bin_dir("pwd") else {
    eprintln!("skipping cwd test: pwd is not in the nix store");
    return;
  };

  let dir =
    std::env::temp_dir().join(format!("mcp-nix-cwd-{}", std::process::id()));
  fs::create_dir_all(&dir).unwrap();
  let expected = fs::canonicalize(&dir)
    .unwrap()
    .to_string_lossy()
    .into_owned();

  let options = RunOptions {
    cwd: Some(dir.to_string_lossy().into_owned()),
    ..Default::default()
  };
  let output = mcp_nix::run_program(&store_dir, "pwd", &options).unwrap();

  assert_eq!(output, expected);
  let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_program_runs_under_bwrap_when_sandboxed() {
  if !bwrap_usable() {
    return;
  }
  let Some(store_dir) = nix_store_dir() else {
    eprintln!("skipping bwrap run_program test: nix is not in the nix store");
    return;
  };
  let sandbox: Vec<String> =
    TEST_BWRAP_ARGS.iter().map(|arg| arg.to_string()).collect();
  let options = RunOptions {
    args: vec!["--version".to_string()],
    sandbox: Some(sandbox),
    ..Default::default()
  };

  let output = mcp_nix::run_program(&store_dir, "nix", &options).unwrap();

  assert!(output.contains("nix"), "unexpected output: {output}");
}

#[test]
fn run_program_under_bwrap_sets_custom_env() {
  if !bwrap_usable() {
    return;
  }
  let Some(store_dir) = store_bin_dir("env") else {
    eprintln!("skipping bwrap environment test: env is not in the nix store");
    return;
  };
  let sandbox: Vec<String> =
    TEST_BWRAP_ARGS.iter().map(|arg| arg.to_string()).collect();
  let options = RunOptions {
    env: HashMap::from([(
      "MCP_NIX_TEST_VAR".to_string(),
      "hello-from-sandbox".to_string(),
    )]),
    sandbox: Some(sandbox),
    ..Default::default()
  };

  let output = mcp_nix::run_program(&store_dir, "env", &options).unwrap();

  assert!(output.contains("HOME=/tmp"), "base HOME missing: {output}");
  assert!(
    output.contains("PATH=/usr/bin:/bin:/nix/var/nix/profiles/default/bin"),
    "base PATH missing: {output}"
  );
  assert!(
    output.contains("MCP_NIX_TEST_VAR=hello-from-sandbox"),
    "custom env missing: {output}"
  );
}

#[test]
fn run_program_under_bwrap_sets_working_directory() {
  if !bwrap_usable() {
    return;
  }
  let Some(store_dir) = store_bin_dir("pwd") else {
    eprintln!("skipping bwrap cwd test: pwd is not in the nix store");
    return;
  };
  let sandbox: Vec<String> =
    TEST_BWRAP_ARGS.iter().map(|arg| arg.to_string()).collect();
  let options = RunOptions {
    cwd: Some("/tmp".to_string()),
    sandbox: Some(sandbox),
    ..Default::default()
  };

  let output = mcp_nix::run_program(&store_dir, "pwd", &options).unwrap();

  assert_eq!(output, "/tmp");
}

#[test]
fn run_package_rejects_program_not_in_allowed_commands() {
  let options = RunOptions {
    allowed_commands: vec!["some-other-program".to_string()],
    ..Default::default()
  };
  let error = mcp_nix::run_package("path:./does-not-matter", "hello", &options)
    .unwrap_err();

  assert!(
    error.to_string().contains("MCP_NIX_COMMANDS"),
    "unexpected error: {error}"
  );
}

#[test]
fn run_package_runs_program_in_allowed_commands() {
  if !bwrap_usable() {
    return;
  }
  for_each_flake(|fixture| {
    let Some(program) = fixture.package_program else {
      return;
    };
    let package_ref = format!("{}#default", fixture.flake.dir.display());
    let sandbox: Vec<String> =
      TEST_BWRAP_ARGS.iter().map(|arg| arg.to_string()).collect();
    let options = RunOptions {
      allowed_commands: vec![program.to_string()],
      sandbox: Some(sandbox),
      ..Default::default()
    };

    let output = mcp_nix::run_package(&package_ref, program, &options).unwrap();

    assert!(
      output.contains("Hello, world!"),
      "unexpected output: {output}"
    );
  });
}

#[test]
fn run_package_builds_package_and_runs_program() {
  if !bwrap_usable() {
    return;
  }
  for_each_flake(|fixture| {
    let package_ref = format!("{}#default", fixture.flake.dir.display());
    let sandbox: Vec<String> =
      TEST_BWRAP_ARGS.iter().map(|arg| arg.to_string()).collect();

    match fixture.package_program {
      Some(program) => {
        let options = RunOptions {
          sandbox: Some(sandbox),
          ..Default::default()
        };
        let output =
          mcp_nix::run_package(&package_ref, program, &options).unwrap();
        assert!(
          output.contains("Hello, world!"),
          "unexpected output: {output}"
        );
      }
      None => {
        let error = mcp_nix::run_package(
          &package_ref,
          "does-not-exist",
          &RunOptions::default(),
        )
        .unwrap_err();
        assert!(
          error
            .to_string()
            .contains("failed to run the built package"),
          "unexpected error: {error}"
        );
      }
    }
  });
}
