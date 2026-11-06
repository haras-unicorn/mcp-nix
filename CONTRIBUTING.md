# Prerequisites

[Nix] is used for managing the development shell.

## Development

To start developing, install [Nix] and run `nix develop .` in the root of the
repository to enter the default development shell of the repository flake. The
shell generates the repository files it owns (see `AGENTS.md`) and links the
locked Rust dependencies, so that [Cargo] works without network access.

The `dev` command in the shell runs the common tasks, for example `dev format`,
`dev check` and `dev run`. Run `dev --help` to list them. `dev check` also runs
the clippy and Rust test checks, and `cargo test` works in the shell too.

## Organization

The Rust code is a Cargo workspace with its crates in `src/crates`, and the Nix
code is in `src/nix`. The documentation is an mdBook in `docs`.

[Nix]: https://nixos.org
[Cargo]: https://doc.rust-lang.org/cargo/
