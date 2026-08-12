# Tools

The `mcp-nix` server exposes the following tools.

## `nix_build`

Build a nix package and return the resulting nix store path.

### Parameters

- `package` (string, required) - a nix package reference, for example
  `nixpkgs#hello`.

### Result

On success the tool returns a text result containing the nix store path of the
built package, for example `/nix/store/...-hello`.

On failure the tool returns a text result with `isError` set and the `nix build`
error output as its content.

### Examples

| Package           | Result                              |
| ----------------- | ----------------------------------- |
| `nixpkgs#hello`   | store path of the `hello` package   |
| `nixpkgs#ripgrep` | store path of the `ripgrep` package |
