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
  the program. The program runs in a sandbox with a clean environment, so these
  supplement the minimal base environment.
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

## `nix_develop`

Enter a nix dev shell and run a command in it, wrapped in a [bubblewrap] sandbox
by default.

### Parameters

- `flake` (string, required) - a flake reference, for example `path:./.` or
  `github:foo/bar#some-devshell`. The dev shell defaults to
  `devShells.<system>.default`.
- `command` (string, required) - the command to run inside the dev shell, for
  example `cargo`.
- `args` (array of strings, optional) - arguments passed to the command.
- `env` (object of strings, optional) - additional environment variables set for
  the command. The command runs in a sandbox with the dev shell environment, so
  these supplement it.
- `cwd` (string, optional) - the working directory for the command. Inside the
  sandbox it must be visible, for example `/tmp`.

### Result

On success the tool returns the standard output of the command.

On failure the tool returns a text result with `isError` set and the error
output as its content.

### Sandboxing

The dev shell environment is captured with `nix print-dev-env` and merged into
the sandbox environment; the command runs inside the [bubblewrap] sandbox with
that environment. The dev shell `shellHook` (and any shell functions) run inside
the sandbox right before the command. Hooks that bootstrap an environment with
network access (for example `uv`) require a sandbox configuration without
`--unshare-net`, see [Sandboxing](./introduction.md).

### Examples

| Flake               | Command | Args    | Result                           |
| ------------------- | ------- | ------- | -------------------------------- |
| `path:./.`          | `cargo` | `build` | standard output of `cargo build` |
| `path:./.#devshell` | `make`  | `test`  | standard output of `make test`   |

[bubblewrap]: https://github.com/containers/bubblewrap
