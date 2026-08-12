use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const FLAKE_NIX: &str = r#"
{
  outputs =
    { self }:
    {
      packages.x86_64-linux.default = derivation {
        name = "mcp-nix-test";
        system = "x86_64-linux";
        builder = "/bin/sh";
        args = [
          "-c"
          "echo mcp-nix-test > \$out"
        ];
      };
      packages.aarch64-linux.default = derivation {
        name = "mcp-nix-test";
        system = "aarch64-linux";
        builder = "/bin/sh";
        args = [
          "-c"
          "echo mcp-nix-test > \$out"
        ];
      };
    };
}
"#;

struct TempFlake {
  dir: PathBuf,
}

impl TempFlake {
  fn new() -> Self {
    let nanos = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .unwrap_or(std::time::Duration::ZERO)
      .as_nanos();

    let dir = std::env::temp_dir()
      .join(format!("mcp-nix-test-{}-{nanos}", process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("flake.nix"), FLAKE_NIX).unwrap();
    Self { dir }
  }
}

impl Drop for TempFlake {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.dir);
  }
}

#[test]
fn build_package_returns_store_path() {
  let flake = TempFlake::new();
  let package_ref = format!("{}#default", flake.dir.display());

  let store_path = mcp_nix::build_package(&package_ref).unwrap();

  assert!(
    store_path.starts_with("/nix/store/"),
    "expected a store path, got: {store_path}"
  );
  assert!(Path::new(&store_path).exists(), "store path does not exist");

  let contents = fs::read_to_string(&store_path).unwrap();
  assert_eq!(contents.trim(), "mcp-nix-test");
}

#[test]
fn build_package_returns_error_for_bad_reference() {
  let flake = TempFlake::new();
  let package_ref = format!("{}#does-not-exist", flake.dir.display());

  let error = mcp_nix::build_package(&package_ref).unwrap_err();

  assert!(
    error.to_string().contains("nix build failed"),
    "unexpected error: {error}"
  );
}
