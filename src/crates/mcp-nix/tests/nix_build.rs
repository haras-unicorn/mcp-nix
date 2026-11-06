//! Integration tests of building packages with a real nix.

mod common;

use common::{
  ECHO_FLAKE, IMPORTED_FIXED_OUTPUT_FLAKE, NO_CHROOT_FLAKE, TempFlake,
  assert_build_refused, assert_leaving_sandbox_refused, assert_nix_builds,
  build_flake_package, fixed_output_flake, for_each_flake,
};

#[test]
fn build_package_returns_store_path() {
  for_each_flake(|fixture| {
    let package_ref = format!("{}#default", fixture.flake.dir.display());

    let store_path =
      mcp_nix::build_package(&common::guard(), &package_ref, false).unwrap();

    assert!(
      store_path.starts_with("/nix/store/"),
      "expected a store path, got: {store_path}"
    );
    assert!(
      std::path::Path::new(&store_path).exists(),
      "store path does not exist"
    );
  });
}

#[test]
fn build_package_builds_in_the_nix_sandbox() {
  assert_nix_builds();
  let Some(guard) = common::sandboxed_guard() else {
    return;
  };
  let flake = TempFlake::new(ECHO_FLAKE);
  let package_ref = format!("{}#default", flake.dir.display());

  let store_path = mcp_nix::build_package(&guard, &package_ref, false).unwrap();

  assert!(
    store_path.starts_with("/nix/store/"),
    "expected a store path, got: {store_path}"
  );
}

#[test]
fn the_nix_sandbox_passes_the_check_before_the_server_starts() {
  assert_nix_builds();
  let Some(guard) = common::sandboxed_guard() else {
    return;
  };

  guard.verify().unwrap();
}

#[test]
fn build_package_refuses_a_fixed_output_derivation_not_in_the_store() {
  assert_nix_builds();
  let flake = fixed_output_flake();
  let package_ref = format!("{}#default", flake.dir.display());

  let error =
    mcp_nix::build_package(&common::guard(), &package_ref, false).unwrap_err();

  assert_build_refused(&error.to_string());
}

#[test]
fn build_package_refuses_what_needs_a_fixed_output_derivation_not_in_the_store()
{
  assert_nix_builds();
  let flake = fixed_output_flake();
  let package_ref = format!("{}#dependent", flake.dir.display());

  let error =
    mcp_nix::build_package(&common::guard(), &package_ref, false).unwrap_err();

  assert_build_refused(&error.to_string());
}

#[test]
fn build_package_builds_what_needs_a_fixed_output_derivation_in_the_store() {
  assert_nix_builds();
  let flake = fixed_output_flake();
  build_flake_package(&flake);
  let package_ref = format!("{}#dependent", flake.dir.display());

  let store_path =
    mcp_nix::build_package(&common::guard(), &package_ref, false).unwrap();

  assert!(
    store_path.starts_with("/nix/store/"),
    "expected a store path, got: {store_path}"
  );
}

#[test]
fn build_package_refuses_a_fixed_output_derivation_evaluation_needs() {
  assert_nix_builds();
  let flake = TempFlake::new(IMPORTED_FIXED_OUTPUT_FLAKE);
  let package_ref = format!("{}#default", flake.dir.display());

  let error =
    mcp_nix::build_package(&common::guard(), &package_ref, false).unwrap_err();

  assert_build_refused(&error.to_string());
}

#[test]
fn build_package_refuses_a_derivation_that_leaves_the_sandbox() {
  assert_nix_builds();
  let flake = TempFlake::new(NO_CHROOT_FLAKE);
  let package_ref = format!("{}#default", flake.dir.display());

  let error =
    mcp_nix::build_package(&common::guard(), &package_ref, false).unwrap_err();

  assert_leaving_sandbox_refused(&error.to_string());
}

#[test]
fn build_package_refuses_a_derivation_that_leaves_the_sandbox_with_structured_attributes()
 {
  assert_nix_builds();
  let flake = TempFlake::new(&common::hook_flake(
    "__structuredAttrs = true; __noChroot = true;",
  ));
  let package_ref = format!("{}#default", flake.dir.display());

  let error =
    mcp_nix::build_package(&common::guard(), &package_ref, false).unwrap_err();

  assert_leaving_sandbox_refused(&error.to_string());
}

#[test]
fn the_hook_of_the_tests_passes_the_check_before_the_server_starts() {
  assert_nix_builds();

  common::guard().verify().unwrap();
}

#[test]
fn build_package_runs_nix_in_the_directory_of_the_guard() {
  assert_nix_builds();
  let flake = TempFlake::new(ECHO_FLAKE);
  let guard = common::guard().in_directory(flake.dir.display().to_string());

  let store_path = mcp_nix::build_package(&guard, ".#default", false).unwrap();

  assert!(
    store_path.starts_with("/nix/store/"),
    "expected a store path, got: {store_path}"
  );
}

#[test]
fn build_package_builds_derivations_without_fixed_output() {
  assert_nix_builds();
  let flake = TempFlake::new(ECHO_FLAKE);
  let package_ref = format!("{}#default", flake.dir.display());

  let store_path =
    mcp_nix::build_package(&common::guard(), &package_ref, false).unwrap();

  assert!(
    store_path.starts_with("/nix/store/"),
    "expected a store path, got: {store_path}"
  );
}

#[test]
fn build_package_returns_error_for_bad_reference() {
  for_each_flake(|fixture| {
    let package_ref = format!("{}#does-not-exist", fixture.flake.dir.display());

    let error = mcp_nix::build_package(&common::guard(), &package_ref, false)
      .unwrap_err();

    assert!(
      error.to_string().contains("nix build failed"),
      "unexpected error: {error}"
    );
  });
}
