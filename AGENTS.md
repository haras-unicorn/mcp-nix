# AGENTS.md

`mcp-nix` is a Rust MCP server that provides nix tooling. It is a Cargo
workspace with a single crate, `mcp-nix`, inside `src`.

## Structure

- `src/mcp-nix/src/lib.rs` - MCP server and tool definitions.
- `src/mcp-nix/src/nix.rs` - wrappers around `nix` commands.
- `src/mcp-nix/src/main.rs` - stdio entry point.
- `src/mcp-nix/tests/nix_build.rs` - integration tests for the `nix` build tool.
- `docs` - mdBook documentation.
- `flake.nix` - flake exposing the development shell, the `mcp-nix` package and
  runnable apps.

## Development

The default development shell (assume you are already running inside it)
provides the following scripts:

- `dev run` - run the MCP server over stdio.
- `dev format` - format the repository.
- `dev lint` - lint and test the repository.

## Tests

The test suite only assumes that `nix` is installed on the system. The
integration tests build a self-contained flake written to a temporary directory,
so they do not require network access.
