use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use mcp_nix::RunOptions;

const TRIVIAL_FLAKE_NIX: &str = r#"
{
  outputs =
    { self }:
    {
      packages.x86_64-linux.default = derivation {
        name = "mcp-nix-run-test";
        system = "x86_64-linux";
        builder = "/bin/sh";
        args = [ "-c" "echo mcp-nix-run-test > \"\$out\"" ];
      };
      packages.aarch64-linux.default = derivation {
        name = "mcp-nix-run-test";
        system = "aarch64-linux";
        builder = "/bin/sh";
        args = [ "-c" "echo mcp-nix-run-test > \"\$out\"" ];
      };
    };
}
"#;

const TEST_BWRAP_ARGS: &[&str] = &[
  "--die-with-parent",
  "--unshare-user",
  "--unshare-ipc",
  "--unshare-pid",
  "--ro-bind",
  "/nix",
  "/nix",
  "--ro-bind",
  "/bin/sh",
  "/bin/sh",
  "--tmpfs",
  "/tmp",
  "--proc",
  "/proc",
  "--dev",
  "/dev",
];

struct TempFlake {
  dir: PathBuf,
}

impl TempFlake {
  fn new(flake_nix: &str) -> Self {
    let nanos = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap_or(std::time::Duration::ZERO)
      .as_nanos();

    let dir = std::env::temp_dir()
      .join(format!("mcp-nix-run-test-{}-{nanos}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("flake.nix"), flake_nix).unwrap();
    Self { dir }
  }
}

impl Drop for TempFlake {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.dir);
  }
}

/// Return the nix store directory containing the given program, or `None`
/// when the program does not resolve into the store.
fn store_bin_dir(program: &str) -> Option<String> {
  let output = process::Command::new("sh")
    .args(["-c", &format!("command -v {program}")])
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }

  let path = String::from_utf8(output.stdout).ok()?;
  let canonical = fs::canonicalize(path.trim()).ok()?;
  let canonical = canonical.to_string_lossy().into_owned();
  if !canonical.starts_with("/nix/store/") {
    return None;
  }

  Path::new(&canonical)
    .parent()?
    .parent()
    .map(|dir| dir.to_string_lossy().into_owned())
}

fn nix_store_dir() -> Option<String> {
  store_bin_dir("nix")
}

fn bwrap_usable() -> bool {
  let version = process::Command::new("bwrap").arg("--version").output();
  let Ok(version) = version else {
    eprintln!("skipping bwrap test: bwrap not found on PATH");
    return false;
  };
  if !version.status.success() {
    eprintln!("skipping bwrap test: bwrap is not functional");
    return false;
  }

  match process::Command::new("bwrap")
    .args(TEST_BWRAP_ARGS)
    .args(["/bin/sh", "-c", "exit 0"])
    .output()
  {
    Ok(output) if output.status.success() => true,
    Ok(output) => {
      eprintln!(
        "skipping bwrap test: sandbox unusable: {}",
        String::from_utf8_lossy(&output.stderr).trim()
      );
      false
    }
    Err(error) => {
      eprintln!("skipping bwrap test: {error}");
      false
    }
  }
}

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
fn run_program_clears_environment() {
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

  assert!(output.contains("HOME=/tmp"), "base HOME missing: {output}");
  assert!(
    output.contains("PATH=/usr/bin:/bin:/nix/var/nix/profiles/default/bin"),
    "base PATH missing: {output}"
  );
  assert!(
    output.contains("MCP_NIX_TEST_VAR=hello-from-env"),
    "custom env missing: {output}"
  );
}

#[test]
fn run_program_sets_working_directory() {
  let Some(store_dir) = store_bin_dir("pwd") else {
    eprintln!("skipping cwd test: pwd is not in the nix store");
    return;
  };

  let dir = std::env::temp_dir().join(format!("mcp-nix-cwd-{}", process::id()));
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
fn run_package_returns_error_for_missing_program() {
  let flake = TempFlake::new(TRIVIAL_FLAKE_NIX);
  let package_ref = format!("{}#default", flake.dir.display());

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
