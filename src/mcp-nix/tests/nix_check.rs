mod common;

use common::{TempFlake, for_each_flake};
use mcp_nix::CheckOptions;

/// A minimal flake with no outputs, which `nix flake check` accepts.
const VALID_FLAKE: &str = r#"
{
  outputs = { self }: { };
}
"#;

/// A flake whose default package refers to itself, which fails to evaluate.
const INVALID_FLAKE: &str = r#"
{
  outputs = { self }: {
    packages.x86_64-linux.default = self.packages.x86_64-linux.default;
  };
}
"#;

#[test]
fn check_succeeds_on_valid_flake() {
  let flake = TempFlake::new(VALID_FLAKE);

  let result = mcp_nix::check_flake(
    &flake.dir.display().to_string(),
    &CheckOptions::default(),
  );

  assert!(result.is_ok(), "expected check to pass, got: {result:?}");
}

#[test]
fn check_fails_on_invalid_flake() {
  let flake = TempFlake::new(INVALID_FLAKE);

  let error = mcp_nix::check_flake(
    &flake.dir.display().to_string(),
    &CheckOptions::default(),
  )
  .unwrap_err();

  assert!(
    error.to_string().contains("nix flake check failed"),
    "unexpected error: {error}"
  );
}

#[test]
fn check_all_systems_and_no_build() {
  for_each_flake(|fixture| {
    let options = CheckOptions {
      all_systems: true,
      no_build: true,
      ..Default::default()
    };

    let result =
      mcp_nix::check_flake(&fixture.flake.dir.display().to_string(), &options);

    assert!(result.is_ok(), "expected check to pass, got: {result:?}");
  });
}
