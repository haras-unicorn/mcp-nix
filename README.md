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

<!-- ANCHOR_END: body -->
