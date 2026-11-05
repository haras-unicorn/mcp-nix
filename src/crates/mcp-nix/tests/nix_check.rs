//! Integration tests of checking flakes with a real nix.

mod common;

use common::{
  TempFlake, assert_build_refused, assert_nix_builds, build_flake_package,
  fixed_output_flake, for_each_flake,
};
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
  assert_nix_builds();
  let flake = TempFlake::new(VALID_FLAKE);

  let result = mcp_nix::check_flake(
    &common::guard(),
    &flake.dir.display().to_string(),
    &CheckOptions::default(),
  );

  assert!(result.is_ok(), "expected check to pass, got: {result:?}");
}

#[test]
fn check_fails_on_invalid_flake() {
  let flake = TempFlake::new(INVALID_FLAKE);

  let error = mcp_nix::check_flake(
    &common::guard(),
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
fn check_refuses_a_fixed_output_derivation_not_in_the_store() {
  assert_nix_builds();
  let flake = fixed_output_flake();

  let error = mcp_nix::check_flake(
    &common::guard(),
    &flake.dir.display().to_string(),
    &CheckOptions::default(),
  )
  .unwrap_err();

  assert_build_refused(&error.to_string());
}

#[test]
fn check_builds_what_needs_a_fixed_output_derivation_in_the_store() {
  assert_nix_builds();
  let flake = fixed_output_flake();
  build_flake_package(&flake);

  let result = mcp_nix::check_flake(
    &common::guard(),
    &flake.dir.display().to_string(),
    &CheckOptions::default(),
  );

  assert!(result.is_ok(), "expected check to pass, got: {result:?}");
}

#[test]
fn check_all_systems_and_no_build() {
  for_each_flake(|fixture| {
    let options = CheckOptions {
      all_systems: true,
      no_build: true,
      ..Default::default()
    };

    let result = mcp_nix::check_flake(
      &common::guard(),
      &fixture.flake.dir.display().to_string(),
      &options,
    );

    assert!(result.is_ok(), "expected check to pass, got: {result:?}");
  });
}
