# MCP Nix

<!-- ANCHOR: body -->

MCP server that provides nix tooling.

## Installation

`mcp-nix` is packaged as a Nix flake. Run it directly without installing:

```sh
nix run github:haras-unicorn/mcp-nix
```

or build the `mcp-nix` binary with:

```sh
nix build github:haras-unicorn/mcp-nix
```

## Usage

`mcp-nix` is an MCP server that speaks the Model Context Protocol over stdio.
Add it as a stdio MCP server to any MCP client, for example:

```json
{
  "mcpServers": {
    "mcp-nix": {
      "command": "nix",
      "args": ["run", "github:haras-unicorn/mcp-nix"]
    }
  }
}
```

If `mcp-nix` is already on your `PATH`, point the client at the binary directly
instead:

```json
{
  "mcpServers": {
    "mcp-nix": {
      "command": "mcp-nix",
      "args": []
    }
  }
}
```

## Sandboxing

Tools that run code from the packages they build (e.g. `nix_run`) and commands
run inside dev shells (e.g. `nix_develop`) are potentially dangerous: `nix run`
is effectively `nix build` followed by running arbitrary code on your machine,
and a dev shell may run shell hooks. These tools first build the package with
`nix build` (or, for `nix_develop`, capture the dev shell environment with
`nix print-dev-env`), which nix already sandboxes, and then run the requested
program wrapped in a [bubblewrap] sandbox. For `nix_develop` the dev shell
environment (environment variables, `PATH`, shell functions and shell hooks)
runs inside the sandbox: the shell hooks are executed inside the sandbox right
before the command, not on your machine.

The sandbox is controlled by the `MCP_NIX_SANDBOX` environment variable:

- if unset, the program runs wrapped in the default sandbox;
- if set to an empty value, the program runs directly, unsandboxed;
- otherwise, its value replaces the entire default argument set and the program
  runs as `bwrap <MCP_NIX_SANDBOX> <command> <args>`.

The default sandbox and the base environment are defined in
[Sandbox configuration](#sandbox-configuration).

The commands that may be executed are restricted by the `MCP_NIX_COMMANDS`
environment variable, a comma-separated list of command file names. Before a
`nix_run` program or a `nix_develop` command runs, its executable file name (for
example `hello` or `cargo`) is checked against the list: when it does not match,
the tool fails with an error and nothing is executed. The check applies whenever
`MCP_NIX_COMMANDS` is set; when the variable is unset, any command is allowed.

The nix store is bound read-only so that the built package, the `nix` client and
dev shell tooling are reachable inside the sandbox. Network is disabled by
default: the package is built (or the dev shell environment captured) outside
the sandbox, so the program run inside does not need network access. Set
`MCP_NIX_SANDBOX` to arguments without `--unshare-net` to allow network access.
Note that dev shell hooks that bootstrap an environment with network access (for
example `uv`) therefore require a sandbox configuration without `--unshare-net`.

To run programs unsandboxed, set `MCP_NIX_SANDBOX` to an empty string:

```json
{
  "mcpServers": {
    "mcp-nix": {
      "command": "nix",
      "args": ["run", "github:haras-unicorn/mcp-nix"],
      "env": {
        "MCP_NIX_SANDBOX": ""
      }
    }
  }
}
```

Bubblewrap requires unprivileged user namespaces, and the sandbox cannot run
inside a nix build sandbox. The `mcp-nix` package bundles both `nix` and
`bubblewrap` on its `PATH`.

### Environment

The `nix` commands run with the environment of the MCP server, so the user's nix
configuration (for example `NIX_CONFIG`, `NIX_PATH` and `SSL_CERT_FILE`)
applies.

Inside the sandbox the environment is cleared with bubblewrap (`--clearenv`) and
set to a minimal set of variables defined in
[Sandbox configuration](#sandbox-configuration).

For `nix_develop` the dev shell environment captured with `nix print-dev-env` is
merged into the sandbox environment (its `PATH` is prepended), and its shell
functions and shell hook run before the command. Additional environment
variables can be provided per call through the `env` parameter.

When the sandbox is disabled (`MCP_NIX_SANDBOX` set to an empty string), the
program or command runs directly and inherits the environment of the MCP server,
with the `env` parameter (and, for `nix_develop`, the dev shell environment)
applied on top.

The working directory can be set per call through the `cwd` parameter. Inside
the sandbox it must be visible: with the default configuration only `/nix`,
`/etc`, `/usr`, `/bin`, `/lib`, `/lib64`, `/tmp`, `/proc` and `/dev` are
visible, so a working directory like `/tmp` works while a path under `/home`
does not.

[bubblewrap]: https://github.com/containers/bubblewrap

<!-- ANCHOR_END: body -->

## Sandbox configuration

The default sandbox arguments and the base environment are defined in
[`sandbox_defaults.rs`](./src/mcp-nix/src/sandbox_defaults.rs).
