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

Tools that run code from the packages they build (e.g. `nix_run`) are
potentially dangerous: `nix run` is effectively `nix build` followed by running
arbitrary code on your machine. These tools first build the package with
`nix build`, which nix already sandboxes, and then run the requested program
wrapped in a [bubblewrap] sandbox.

The sandbox is controlled by the `MCP_NIX_SANDBOX` environment variable:

- if unset, the program runs wrapped in the default sandbox;
- if set to an empty value, the program runs directly, unsandboxed;
- otherwise, its value replaces the entire default argument set and the program
  runs as `bwrap <MCP_NIX_SANDBOX> <store path>/bin/<program>`.

The default sandbox is:

```text
--die-with-parent --unshare-user --unshare-ipc --unshare-pid --unshare-net
--ro-bind /nix /nix --ro-bind /etc /etc
--ro-bind /usr /usr --ro-bind /bin /bin --ro-bind /lib /lib --ro-bind /lib64 /lib64
--tmpfs /tmp --tmpfs /home --tmpfs /run --proc /proc --dev /dev
```

The nix store is bound read-only so that the built package and the `nix` client
are reachable inside the sandbox. Network is disabled by default: the package is
built with `nix build` outside the sandbox, so the program run inside does not
need network access. Set `MCP_NIX_SANDBOX` to arguments without `--unshare-net`
to allow network access.

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

Tools that run code from the packages they build (e.g. `nix_run`) run it with a
cleared environment: they do not inherit the environment of the MCP server. Only
a minimal set of variables is set:

```text
HOME=/tmp
LANG=C.UTF-8
PATH=/usr/bin:/bin:/nix/var/nix/profiles/default/bin
TMPDIR=/tmp
```

Additional environment variables and the working directory can be provided per
call through the `env` and `cwd` parameters of `nix_run`. Inside the sandbox the
working directory must be visible: with the default configuration only `/nix`,
`/etc`, `/usr`, `/bin`, `/lib`, `/lib64`, `/tmp`, `/proc` and `/dev` are
visible, so a working directory like `/tmp` works while a path under `/home`
does not.

[bubblewrap]: https://github.com/containers/bubblewrap

<!-- ANCHOR_END: body -->
