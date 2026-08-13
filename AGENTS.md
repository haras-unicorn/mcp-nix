# AGENTS.md

`mcp-nix` is a Rust MCP server that provides nix tooling. It is a Cargo
workspace with a single crate, `mcp-nix`, inside `src`.

## Structure

- `src/mcp-nix/src/lib.rs` - MCP server and tool definitions.
- `src/mcp-nix/src/nix.rs` - wrappers around `nix` commands.
- `src/mcp-nix/src/main.rs` - stdio entry point.
- `docs` - mdBook documentation.
- `flake.nix` - flake exposing the development shell, the `mcp-nix` package and
  runnable apps.

## Development

The default development shell (assume you are already running inside it)
provides the following scripts:

- `dev run` - run the MCP server over stdio.
- `dev format` - format the repository.
- `dev lint` - lint and test the repository.
- `dev test` - check clippy warnings and run rust tests.

## Tests

The test suite only assumes that `nix` is installed on the system. The
integration tests build self-contained flakes written to temporary directories,
so they do not require network access. The dev shell tests pin their nixpkgs
input to the revision locked in the repository's own `flake.lock`, which is
already cached when running inside the default development shell.
