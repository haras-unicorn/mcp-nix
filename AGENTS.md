# AGENTS.md

`mcp-nix` is an MCP server that provides nix tooling over stdio: building,
running, checking and logging, with programs run inside a bubblewrap sandbox. It
is a Rust Cargo workspace built on
[haraspkgs](https://github.com/haras-unicorn/haraspkgs).

## Structure

- `src/crates` - the Cargo workspace members. Today a single crate, `mcp-nix`,
  that is both a library (`lib.rs` only declares modules and the prelude; the
  server, `nix`, `sandbox` and `hook` live in their own modules) and two
  binaries: `mcp-nix` (`args`, `config`, `log` and `run` around `main`) and
  `mcp-nix-hook` in `src/bin`, the program `nix` runs before each build.
- `src/crates/*/tests` - integration tests that drive a real `nix`.
- `src/nix/tests` - NixOS VM tests, for example the one of `mcp-nix-podman`.
  They are flake checks, so `nix flake check` runs them, and a single one can be
  built as `checks.<system>.nixos-test-<name>`.
- `src/nix` - every Nix file, each a flake-parts module for haraspkgs:
  `systems.nix`, the package overlays in `packages/`, the dev shell in `dev/`
  and the VM tests in `tests/`.
- `docs` - the mdBook. It includes parts of `README.md` and of the Rust sources
  through anchors, so moving either means updating the includes.
- `Cargo.toml` and `clippy.toml` at the root hold the shared package metadata,
  the lints and the profiles.

## Development

Enter the default dev shell (`nix develop`, or direnv). It provides the Rust
toolchain, the formatters and the `dev` CLI; run `dev --help` for the commands.

- `dev format` formats the repository, `dev check` runs the checks. Clippy with
  `-D warnings` and the Rust tests are checks, so they also run in
  `nix flake check`.
- `dev run` runs the server over stdio, with the locally built hook.

The tests only need `nix` on the system. They build self-contained flakes in
temporary directories, and pin nixpkgs to the revision locked in `flake.lock`
through `MCP_NIX_NIXPKGS_REV`, which the dev shell sets.

## Configuration

The binary reads its settings (see `Config` in `config.rs`) from defaults, then
a TOML file (`--config`, or `mcp-nix.toml` in the current directory), then
`MCP_NIX__<KEY>` environment variables. The library itself never reads the
environment.

Bubblewrap is run through configurable commands (`bwrap_command` for the
programs, `nix_bwrap_command` for `nix`), so it can be wrapped in another
program, for example `nsenter`.

Every `nix` command the server runs goes through the `BuildGuard` in `nix.rs`,
which builds on `hook.rs` (the decision of the hook, and `PreBuildHook`, how
`nix` is made to use it) and `sandbox.rs` (all of the bubblewrap logic,
including `NixSandbox`). It does two things:

- It gives `nix` the `pre-build-hook` setting, pointing at `mcp-nix-hook`
  (`pre_build_hook`), which refuses to build fixed-output derivations. That is
  why the server needs a trusted nix user. `allow_fod` turns the hook off.
- It runs `nix` in its own bubblewrap sandbox without network
  (`nix_bwrap_args`), because the hook cannot see what evaluation fetches. The
  daemon socket is the one way out of it, and the `bwrap_args` of the programs
  must not have it.

The guard checks both at startup. `docs/tools.md` has the details.

## Generated files

The dev shell owns these, and entering it rewrites them. Edit the dev module in
`src/nix/dev/shell.nix` and not the files: `.gitignore`, `.gitattributes`,
`.envrc`, `LICENSE.md`, `CODE_OF_CONDUCT.md` and `.github/workflows`. Tool
configuration (EditorConfig, Prettier, cspell, the editor's language servers) is
delivered by the shell and is not committed. `.cargo/config.toml` is linked into
the checkout from the store and is git-ignored.

## Dependencies and building

Cargo has no network in the sandbox, so the dev shell vendors the locked
dependencies with crane and links the vendor config as `.cargo/config.toml`.
Because of that, update `Cargo.lock` outside the shell (the update workflow
does). The package is built with crane for the musl target as static binaries
and wrapped so that `nix`, `bubblewrap` and `bash` are on the `PATH` of
`mcp-nix` and `nix` is on the `PATH` of `mcp-nix-hook`. The wrapper of `mcp-nix`
also defaults `MCP_NIX__PRE_BUILD_HOOK` to the hook in the same package.

The package overlay in `src/nix/packages/mcp-nix.nix` uses crane and
rust-overlay from haraspkgs' inputs directly. This deviates from using
haraspkgs' Rust helpers because those live in an overlay that haraspkgs does not
export to downstream flakes.

## Releases

release-plz opens release pull requests and tags from conventional commits. A
tag triggers the release workflow, which attaches static binaries (`mcp-nix` and
`mcp-nix-hook`) for each architecture to the GitHub release.
