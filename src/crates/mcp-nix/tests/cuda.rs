//! Integration tests of the GPU in the sandboxes of the programs.
//!
//! They need an NVIDIA GPU with its driver loaded, which CI rarely has, so
//! they skip on a machine without one and do not fail.

#![allow(clippy::print_stderr, reason = "a skipped test says why on stderr")]

mod common;

use std::collections::HashMap;

use common::{assert_nix_builds, bwrap_usable, python_flake};
use mcp_nix::{
  DEFAULT_SANDBOX_ARGS, RunOptions, cuda_sandbox_args, parse_sandbox_args,
};

/// Where NixOS links the userspace driver, and where the programs of the nix
/// store look for `libcuda`, which the sandbox of a program does not set.
const DRIVER_LIBRARY_PATH: &str = "/run/opengl-driver/lib";

/// Initializes the driver API of CUDA through `libcuda`, which needs no CUDA
/// toolkit, and prints how many devices it sees.
const DEVICE_COUNT_PROGRAM: &str = "\
import ctypes
cuda = ctypes.CDLL('libcuda.so.1')
assert cuda.cuInit(0) == 0, 'cuInit failed'
count = ctypes.c_int()
assert cuda.cuDeviceGetCount(ctypes.byref(count)) == 0, 'no device count'
print(count.value)";

#[test]
fn the_default_sandbox_with_the_cuda_arguments_sees_the_gpu() {
  let Some(cuda) = cuda_sandbox_args() else {
    eprintln!("skipping cuda test: there is no NVIDIA GPU");
    return;
  };
  if !bwrap_usable() {
    return;
  }

  let output =
    count_devices(&format!("{DEFAULT_SANDBOX_ARGS} {cuda}")).unwrap();

  let devices: u32 = output.parse().unwrap();
  assert!(devices >= 1, "unexpected output: {output}");
}

#[test]
fn the_default_sandbox_without_the_cuda_arguments_does_not() {
  if cuda_sandbox_args().is_none() {
    eprintln!("skipping cuda test: there is no NVIDIA GPU");
    return;
  }
  if !bwrap_usable() {
    return;
  }

  let result = count_devices(DEFAULT_SANDBOX_ARGS);

  assert!(result.is_err(), "unexpected output: {result:?}");
}

fn count_devices(sandbox: &str) -> Result<String, mcp_nix::NixError> {
  assert_nix_builds();
  let flake = python_flake();
  let options = RunOptions {
    args: vec!["-c".to_string(), DEVICE_COUNT_PROGRAM.to_string()],
    env: HashMap::from([(
      "LD_LIBRARY_PATH".to_string(),
      DRIVER_LIBRARY_PATH.to_string(),
    )]),
    sandbox: parse_sandbox_args(sandbox),
    ..Default::default()
  };

  mcp_nix::run_package(
    &common::guard(),
    &format!("{}#default", flake.dir.display()),
    "python3",
    &options,
  )
}
