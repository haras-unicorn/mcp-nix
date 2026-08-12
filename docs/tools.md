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

## `nix_run`

Build a nix package and run one of its programs, wrapped in a [bubblewrap]
sandbox by default.

### Parameters

- `package` (string, required) - a nix package reference, for example
  `nixpkgs#hello`.
- `program` (string, required) - the program to run from the built package, for
  example `hello`.
- `args` (array of strings, optional) - arguments passed to the program.
- `env` (object of strings, optional) - additional environment variables set for
  the program. The program runs with a cleared environment, so these supplement
  the minimal base environment.
- `cwd` (string, optional) - the working directory for the program. Inside the
  sandbox it must be visible, for example `/tmp`.

### Result

On success the tool returns the standard output of the program.

On failure the tool returns a text result with `isError` set and the error
output as its content.

### Sandboxing

See [Sandboxing](./introduction.md) for how the program is wrapped in a
bubblewrap sandbox and how its environment is set.

### Examples

| Package         | Program | Args | Result          |
| --------------- | ------- | ---- | --------------- |
| `nixpkgs#hello` | `hello` | -    | `Hello, world!` |

[bubblewrap]: https://github.com/containers/bubblewrap
