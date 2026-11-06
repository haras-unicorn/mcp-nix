//! Integration tests of the three network boundaries of the tools.
//!
//! The tools must never give the caller the network. Each test serves a file
//! from a mock server on the loopback interface and expects that nothing asks
//! for it, and then runs the same thing without the boundary, to show that the
//! mock server was within reach and that the first run failed because of the
//! boundary and not for another reason.
//!
//! The guarded run goes first, because what the control fetches ends up in the
//! store and would hide a leak from then on.

#![allow(clippy::print_stderr, reason = "a skipped test says why on stderr")]

mod common;

use std::collections::HashMap;

use common::{
  MockServer, TempFlake, assert_build_refused, assert_nix_builds,
  build_flake_package, bwrap_usable, nix_store_dir, sandboxed_guard,
  unique_suffix,
};
use mcp_nix::RunOptions;

/// What the mock server serves, and the hash of it in the flakes below.
const PAYLOAD: &str = "mcp-nix-test\n";
const PAYLOAD_HASH: &str =
  "sha256-E3MwNTPc5Nd2XQFqub3Vj6dsMkn79RP0K1IQaXNpUX4=";

/// A flake with an input that nix fetches when it evaluates the flake. The
/// input is no flake, so any file will do.
const INPUT_FLAKE: &str = r#"
{
  inputs.payload = {
    url = "file+http://@address@/payload-@unique@";
    flake = false;
  };
  outputs = { self, payload }: {
    packages.x86_64-linux.default = derivation {
      name = "mcp-nix-network-input";
      system = "x86_64-linux";
      builder = "/bin/sh";
      args = [ "-c" "echo ${payload} > \"$out\"" ];
    };
  };
}
"#;

/// A flake that fetches a file while nix evaluates it.
const FETCHURL_FLAKE: &str = r#"
{
  outputs = { self }:
    let
      payload = builtins.fetchurl {
        url = "http://@address@/payload-@unique@";
        sha256 = "@hash@";
      };
    in
    {
      packages.x86_64-linux.default = derivation {
        name = "mcp-nix-network-fetchurl";
        system = "x86_64-linux";
        builder = "/bin/sh";
        args = [ "-c" "echo ${payload} > \"$out\"" ];
      };
    };
}
"#;

/// A flake with a fixed-output derivation that nix itself downloads, so that
/// the build needs nothing but nix to reach the network.
const DOWNLOAD_FLAKE: &str = r#"
{
  outputs = { self }: {
    packages.x86_64-linux.default = derivation {
      name = "mcp-nix-network-download-@unique@";
      system = "builtin";
      builder = "builtin:fetchurl";
      url = "http://@address@/payload-@unique@";
      unpack = "";
      executable = "";
      outputHashMode = "flat";
      outputHash = "@hash@";
    };
  };
}
"#;

#[test]
fn nix_cannot_fetch_while_it_evaluates() {
  assert_nix_builds();
  let Some(guard) = sandboxed_guard() else {
    return;
  };

  for template in [INPUT_FLAKE, FETCHURL_FLAKE] {
    let server = MockServer::start();
    server.serve(PAYLOAD);
    let flake = TempFlake::new(&fill(template, &server));
    let package_ref = format!("{}#default", flake.dir.display());

    let result = mcp_nix::build_package(&guard, &package_ref, false);

    assert_eq!(
      server.requests(),
      0,
      "the nix of the tool fetched while it evaluated, result: {result:?}"
    );
    let error = result.unwrap_err().to_string();
    assert!(
      error.contains("unable to download"),
      "expected nix to try the download and fail, got: {error}"
    );

    build_flake_package(&flake);
    assert!(
      server.requests() > 0,
      "plain nix did not reach the server, so the test shows nothing"
    );
  }
}

#[test]
fn the_nix_daemon_does_not_build_a_fixed_output_derivation() {
  assert_nix_builds();
  let server = MockServer::start();
  server.serve(PAYLOAD);
  let flake = TempFlake::new(&fill(DOWNLOAD_FLAKE, &server));
  let package_ref = format!("{}#default", flake.dir.display());

  let result = mcp_nix::build_package(&common::guard(), &package_ref, false);

  assert_eq!(
    server.requests(),
    0,
    "the build downloaded a file, result: {result:?}"
  );
  assert_build_refused(&result.unwrap_err().to_string());

  build_flake_package(&flake);
  assert!(
    server.requests() > 0,
    "plain nix did not reach the server, so the test shows nothing"
  );
}

#[test]
fn nix_cannot_reach_the_network_from_the_sandbox_of_run() {
  assert_nix_builds();
  let Some(store_dir) = nix_store_dir() else {
    eprintln!("skipping sandbox network test: nix is not in the nix store");
    return;
  };
  if !bwrap_usable() {
    return;
  }
  let server = MockServer::start();
  server.serve(PAYLOAD);
  let scratch = TempFlake::new("");
  let scratch_dir = scratch.dir.display().to_string();
  let sandbox = mcp_nix::parse_sandbox_args(mcp_nix::DEFAULT_SANDBOX_ARGS).map(
    |mut args| {
      let dir = scratch_dir.as_str();
      args.extend(["--bind", dir, dir].map(str::to_string));
      args
    },
  );
  let fetch = fetch_command(&server, &scratch_dir);

  let sandboxed = RunOptions {
    args: fetch.clone(),
    env: HashMap::from([("HOME".to_string(), scratch_dir.clone())]),
    sandbox,
    ..Default::default()
  };
  let result = mcp_nix::run_program(&store_dir, "nix", &sandboxed);

  assert_eq!(
    server.requests(),
    0,
    "nix in the sandbox fetched a file, result: {result:?}"
  );
  let error = result.unwrap_err().to_string();
  assert!(
    error.contains("unable to download"),
    "expected nix to try the download and fail, got: {error}"
  );

  let plain = RunOptions {
    args: fetch,
    env: HashMap::from([("HOME".to_string(), scratch_dir)]),
    ..Default::default()
  };
  mcp_nix::run_program(&store_dir, "nix", &plain).unwrap();
  assert!(
    server.requests() > 0,
    "plain nix did not reach the server, so the test shows nothing"
  );
}

/// The arguments for nix to fetch the payload into a store of its own in the
/// given directory, which both the sandbox and plain nix can write to.
fn fetch_command(server: &MockServer, scratch_dir: &str) -> Vec<String> {
  [
    "--extra-experimental-features",
    "nix-command",
    "--store",
    &format!("{scratch_dir}/store"),
    "eval",
    "--impure",
    "--raw",
    "--expr",
    &format!(
      "builtins.fetchurl \"http://{}/payload-{}\"",
      server.address(),
      unique_suffix()
    ),
  ]
  .map(str::to_string)
  .to_vec()
}

/// A flake template with the address of the server, the hash of the payload
/// and a name nothing else has used filled in.
fn fill(template: &str, server: &MockServer) -> String {
  template
    .replace("@address@", &server.address())
    .replace("@hash@", PAYLOAD_HASH)
    .replace("@unique@", &unique_suffix())
}
