//! Entry point of the `mcp-nix` binary.

mod args;
mod config;
mod log;
mod run;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args = args::Args::parse();
  let config = config::Config::load(args.config.as_deref())?;
  log::init(&config.log_level)?;
  run::run(config).await
}
