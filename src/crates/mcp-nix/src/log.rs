//! Logging of the `mcp-nix` binary.

use anyhow::Context;
use tracing::level_filters::LevelFilter;

/// Log events as JSON on stderr, at or above the given level. Standard output
/// belongs to the MCP protocol.
pub fn init(level: &str) -> anyhow::Result<()> {
  tracing_subscriber::fmt()
    .json()
    .with_max_level(parse_level(level)?)
    .with_writer(std::io::stderr)
    .try_init()
    .map_err(|error| anyhow::anyhow!(error))
}

fn parse_level(level: &str) -> anyhow::Result<LevelFilter> {
  level
    .parse()
    .with_context(|| format!("invalid log level: {level}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_level_accepts_known_levels() {
    assert_eq!(parse_level("info").unwrap(), LevelFilter::INFO);
    assert_eq!(parse_level("debug").unwrap(), LevelFilter::DEBUG);
    assert_eq!(parse_level("off").unwrap(), LevelFilter::OFF);
  }

  #[test]
  fn parse_level_rejects_unknown_levels() {
    assert!(parse_level("loud").is_err());
  }
}
