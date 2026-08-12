mod common;

use common::for_each_flake;

#[test]
fn build_package_returns_store_path() {
  for_each_flake(|fixture| {
    let package_ref = format!("{}#default", fixture.flake.dir.display());

    let store_path = mcp_nix::build_package(&package_ref).unwrap();

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
fn build_package_returns_error_for_bad_reference() {
  for_each_flake(|fixture| {
    let package_ref = format!("{}#does-not-exist", fixture.flake.dir.display());

    let error = mcp_nix::build_package(&package_ref).unwrap_err();

    assert!(
      error.to_string().contains("nix build failed"),
      "unexpected error: {error}"
    );
  });
}
