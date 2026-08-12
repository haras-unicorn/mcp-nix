//! Shared helpers for the integration tests.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

/// The nixpkgs revision locked in the repository's `flake.lock`, injected by
/// the `MCP_NIX_NIXPKGS_REV` environment variable set in the dev shell and the
/// package build, so that it can never drift from the lock file.
const NIXPKGS_REV: &str = env!("MCP_NIX_NIXPKGS_REV");

/// Bubblewrap arguments used by the sandboxed tests.
///
/// Note that the default sandbox of the server also disables the network
/// (`--unshare-net`); the tests only add it explicitly when exercising the
/// network.
pub const TEST_BWRAP_ARGS: &[&str] = &[
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

/// A self-contained flake written to a temporary directory.
pub struct TempFlake {
  /// The directory containing the flake.
  pub dir: PathBuf,
}

impl TempFlake {
  /// Write the given flake contents to a new temporary directory.
  pub fn new(flake_nix: &str) -> Self {
    let nanos = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap_or(std::time::Duration::ZERO)
      .as_nanos();

    let dir = std::env::temp_dir()
      .join(format!("mcp-nix-test-{}-{nanos}", process::id()));
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
pub fn store_bin_dir(program: &str) -> Option<String> {
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

/// The nix store directory of the `nix` binary, when it resolves into the
/// store.
pub fn nix_store_dir() -> Option<String> {
  store_bin_dir("nix")
}

/// Whether bubblewrap is installed and the test sandbox can be created.
pub fn bwrap_usable() -> bool {
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

/// Escape a string for embedding in a nix double-quoted string.
fn nix_escape(value: &str) -> String {
  value
    .replace('\\', "\\\\")
    .replace('"', "\\\"")
    .replace('$', "\\$")
}

/// A self-contained flake exposing a package and a dev shell, both built as
/// raw derivations. The package is built with `/bin/sh`; the dev shell is
/// built with the store `bash` so that `nix print-dev-env` accepts it.
pub fn flake_nix(build_command: &str) -> String {
  let bash = store_bin_dir("bash").unwrap_or_default();
  let build = nix_escape(build_command);
  format!(
    r#"
{{
  outputs =
    {{ self }}:
    {{
      packages.x86_64-linux.default = derivation {{
        name = "mcp-nix-test";
        system = "x86_64-linux";
        builder = "/bin/sh";
        args = [ "-c" "{build}" ];
      }};
      packages.aarch64-linux.default = derivation {{
        name = "mcp-nix-test";
        system = "aarch64-linux";
        builder = "/bin/sh";
        args = [ "-c" "{build}" ];
      }};
      devShells.x86_64-linux.default = derivation {{
        name = "mcp-nix-develop-test";
        system = "x86_64-linux";
        outputs = [ "out" ];
        builder = "{bash}/bin/bash";
        args = [ "-c" "{build}" ];
        shellHook = "export MCP_NIX_SHELL_HOOK=ran";
        MCP_NIX_DEVELOP_TEST = "from-dev-shell";
      }};
      devShells.aarch64-linux.default = derivation {{
        name = "mcp-nix-develop-test";
        system = "aarch64-linux";
        outputs = [ "out" ];
        builder = "{bash}/bin/bash";
        args = [ "-c" "{build}" ];
        shellHook = "export MCP_NIX_SHELL_HOOK=ran";
        MCP_NIX_DEVELOP_TEST = "from-dev-shell";
      }};
    }};
}}
"#
  )
}

/// A flake whose default package is built with nixpkgs tools (`hello`) and
/// whose dev shell is a nixpkgs `mkShell` with `hello` available. The dev
/// shell sets the same environment markers as [`flake_nix`].
pub fn nixpkgs_flake() -> String {
  format!(
    r#"
{{
  inputs = {{
    nixpkgs.url = "github:nixos/nixpkgs/{NIXPKGS_REV}";
  }};
  outputs =
    {{ self, nixpkgs }}:
    {{
      packages.x86_64-linux.default = nixpkgs.legacyPackages.x86_64-linux.hello;
      packages.aarch64-linux.default = nixpkgs.legacyPackages.aarch64-linux.hello;
      devShells.x86_64-linux.default = nixpkgs.legacyPackages.x86_64-linux.mkShell {{
        packages = [ nixpkgs.legacyPackages.x86_64-linux.hello ];
        shellHook = "export MCP_NIX_SHELL_HOOK=ran";
        MCP_NIX_DEVELOP_TEST = "from-dev-shell";
      }};
      devShells.aarch64-linux.default = nixpkgs.legacyPackages.aarch64-linux.mkShell {{
        packages = [ nixpkgs.legacyPackages.aarch64-linux.hello ];
        shellHook = "export MCP_NIX_SHELL_HOOK=ran";
        MCP_NIX_DEVELOP_TEST = "from-dev-shell";
      }};
    }};
}}
"#
  )
}

/// Build the dev shell environment of the given flake ahead of time, so that
/// `nix print-dev-env` does not have to build it at test time.
///
/// The raw-derivation dev shell is built with a store `bash` whose dynamic
/// dependencies are not visible inside the default nix build sandbox; the
/// build sandbox paths are therefore augmented to expose the store `bash`.
/// Returns whether the environment could be built; tests skip when it cannot.
pub fn prime_dev_shell(flake_dir: &Path, sandbox_paths: Option<&str>) -> bool {
  let mut command = process::Command::new("nix");
  command.args(["print-dev-env", &flake_dir.display().to_string(), "--json"]);
  if let Some(paths) = sandbox_paths {
    command.args(["--option", "sandbox-paths", paths]);
  }
  command.stdout(process::Stdio::null());
  command.stderr(process::Stdio::null());

  match command.status() {
    Ok(status) if status.success() => true,
    Ok(_) => {
      eprintln!(
        "skipping flake test: could not build the dev shell environment"
      );
      false
    }
    Err(error) => {
      eprintln!("skipping flake test: failed to run nix: {error}");
      false
    }
  }
}

/// A primed flake exposing the raw-derivation package and dev shell, or `None`
/// when the dev shell environment could not be built.
fn raw_dev_shell_flake() -> Option<TempFlake> {
  let flake = TempFlake::new(&flake_nix("echo mcp-nix-test > \"$out\""));
  let bash = store_bin_dir("bash")?;
  let sandbox_paths = format!("bash={bash}/bin/bash");
  if prime_dev_shell(&flake.dir, Some(&sandbox_paths)) {
    Some(flake)
  } else {
    None
  }
}

/// A primed flake exposing a nixpkgs package and dev shell, or `None` when the
/// dev shell environment could not be built.
fn nixpkgs_dev_shell_flake() -> Option<TempFlake> {
  let flake = TempFlake::new(&nixpkgs_flake());
  if prime_dev_shell(&flake.dir, None) {
    Some(flake)
  } else {
    None
  }
}

/// A primed flake fixture exposing a package (`#default`) and a dev shell.
pub struct Fixture {
  /// The flake.
  pub flake: TempFlake,
  /// A program provided by the built package, when it has one.
  pub package_program: Option<&'static str>,
}

/// All flake fixtures the tools are tested against: a raw-derivation flake and
/// a flake built with nixpkgs tools. Flakes whose dev shell environment could
/// not be built are skipped.
pub fn fixtures() -> Vec<Fixture> {
  let mut fixtures = Vec::new();
  if let Some(flake) = raw_dev_shell_flake() {
    fixtures.push(Fixture {
      flake,
      package_program: None,
    });
  }
  if let Some(flake) = nixpkgs_dev_shell_flake() {
    fixtures.push(Fixture {
      flake,
      package_program: Some("hello"),
    });
  }
  fixtures
}

/// Run the given test against every fixture in [`fixtures`].
pub fn for_each_flake(mut test: impl FnMut(&Fixture)) {
  for fixture in fixtures() {
    test(&fixture);
  }
}
