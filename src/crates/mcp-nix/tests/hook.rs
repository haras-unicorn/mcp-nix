//! Integration tests of the decision of the pre-build hook, on the derivations
//! a real nix instantiates and the output of its `nix derivation show`, so
//! that the hook cannot drift from what nix really writes.
//!
//! The tests run the `mcp-nix-hook` binary on a derivation file, the way nix
//! does before a build. Building is not needed, so they do not depend on what
//! nix itself refuses on the machine they run on.

#![allow(
  clippy::expect_used,
  reason = "helpers of tests fail by panicking, like the tests"
)]

mod common;

use std::process::{Command, Output};

use common::{TempFlake, assert_nix_builds, hook_flake};

#[test]
fn a_plain_derivation_is_allowed() {
  let output = run_hook("");

  assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn leaving_the_sandbox_is_refused() {
  let output = run_hook("__noChroot = true;");

  assert_refused(&output, "__noChroot");
}

#[test]
fn leaving_the_sandbox_with_structured_attributes_is_refused() {
  let output = run_hook("__structuredAttrs = true; __noChroot = true;");

  assert_refused(&output, "__noChroot");
}

#[test]
fn staying_in_the_sandbox_with_structured_attributes_is_allowed() {
  let output = run_hook("__structuredAttrs = true; __noChroot = false;");

  assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn requiring_recursive_nix_is_refused() {
  let output =
    run_hook(r#"requiredSystemFeatures = [ "kvm" "recursive-nix" ];"#);

  assert_refused(&output, "recursive-nix");
}

#[test]
fn requiring_recursive_nix_with_structured_attributes_is_refused() {
  let output = run_hook(
    r#"__structuredAttrs = true; requiredSystemFeatures = [ "recursive-nix" ];"#,
  );

  assert_refused(&output, "recursive-nix");
}

#[test]
fn requiring_other_system_features_is_allowed() {
  let output = run_hook(r#"requiredSystemFeatures = [ "kvm" ];"#);

  assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn a_fixed_output_derivation_is_refused() {
  let output = run_hook(
    r#"outputHashMode = "flat";
       outputHash = "sha256-E3MwNTPc5Nd2XQFqub3Vj6dsMkn79RP0K1IQaXNpUX4=";"#,
  );

  assert_refused(&output, "fixed-output");
}

#[test]
fn a_fixed_output_derivation_with_structured_attributes_is_refused() {
  let output = run_hook(
    r#"__structuredAttrs = true;
       outputHashMode = "flat";
       outputHash = "sha256-E3MwNTPc5Nd2XQFqub3Vj6dsMkn79RP0K1IQaXNpUX4=";"#,
  );

  assert_refused(&output, "fixed-output");
}

/// Instantiate a derivation with the given extra attributes and run the hook
/// on its derivation file.
fn run_hook(attributes: &str) -> Output {
  assert_nix_builds();
  let flake = TempFlake::new(&hook_flake(attributes));
  let derivation = derivation_file(&flake);

  Command::new(env!("CARGO_BIN_EXE_mcp-nix-hook"))
    .arg(derivation)
    .output()
    .expect("failed to run the hook")
}

/// The derivation file of the default package of the flake.
fn derivation_file(flake: &TempFlake) -> String {
  let output = Command::new("nix")
    .args([
      "eval",
      "--raw",
      &format!(
        "{}#packages.x86_64-linux.default.drvPath",
        flake.dir.display()
      ),
    ])
    .output()
    .expect("failed to spawn nix");
  assert!(
    output.status.success(),
    "nix cannot instantiate the derivation: {}",
    stderr(&output)
  );

  String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn assert_refused(output: &Output, reason: &str) {
  assert!(
    !output.status.success(),
    "the hook allowed the derivation: {}",
    stderr(output)
  );
  assert!(
    stderr(output).contains(reason),
    "expected the reason {reason:?}, got: {}",
    stderr(output)
  );
}

fn stderr(output: &Output) -> String {
  String::from_utf8_lossy(&output.stderr).into_owned()
}
