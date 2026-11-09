//! mcp-nix - MCP server that provides nix tooling.

pub mod hook;
mod nix;
mod sandbox;
mod sandbox_defaults;
mod server;

pub use nix::{
  BuildGuard, CheckOptions, LogOptions, NixError, build_package, check_flake,
  develop, fetch_log, run_package,
};
pub use sandbox::{
  BindRoot, DEFAULT_BWRAP_COMMAND, DEFAULT_CUDA_SANDBOX_ARGS,
  DEFAULT_NIX_SANDBOX_ARGS, DEFAULT_SANDBOX_ARGS, NVIDIA_GPU_DEVICE,
  RunOptions, bind_root_args, cuda_sandbox_args, parse_bwrap_command,
  parse_sandbox_args, run_program,
};
pub use server::{
  BuildParams, CheckParams, DevelopParams, LogParams, NixServer, RunParams,
  ServerSettings,
};

/// What most users of the library need.
pub mod prelude {
  pub use crate::{
    BuildGuard, CheckOptions, LogOptions, NixError, NixServer, RunOptions,
    ServerSettings,
  };
}
