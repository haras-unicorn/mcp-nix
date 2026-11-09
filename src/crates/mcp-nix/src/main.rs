//! Entry point of the `mcp-nix` binary.

mod args;
mod config;
mod log;
mod run;
mod show;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args = args::Args::parse();
  if let Some(command) = &args.command {
    return show::show(command, args.config.as_deref());
  }

  let config = config::Config::load(args.config.as_deref())?;
  log::init(&config.log_level)?;
  run::run(config).await
}
