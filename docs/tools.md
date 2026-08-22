# Tools

The `mcp-nix` server exposes the following tools.

## `nix_build`

Build a nix package and return the resulting nix store path.

### Parameters

- `package` (string, required) - a nix package reference, for example
  `nixpkgs#hello`.
- `show_trace` (boolean, optional) - pass `--show-trace` to `nix build` so that
  evaluation errors include the full stack trace.

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
- `show_trace` (boolean, optional) - pass `--show-trace` to the `nix build` step
  so that evaluation errors include the full stack trace.
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
bubblewrap sandbox and how its environment is set. When the `MCP_NIX_COMMANDS`
environment variable is set, the `program` must be listed in it for the tool to
run.

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
- `show_trace` (boolean, optional) - pass `--show-trace` to `nix print-dev-env`
  so that evaluation errors include the full stack trace.
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
`--unshare-net`, see [Sandboxing](./introduction.md). When the
`MCP_NIX_COMMANDS` environment variable is set, the `command` must be listed in
it for the tool to run.

### Examples

| Flake               | Command | Args    | Result                           |
| ------------------- | ------- | ------- | -------------------------------- |
| `path:./.`          | `cargo` | `build` | standard output of `cargo build` |
| `path:./.#devshell` | `make`  | `test`  | standard output of `make test`   |

## `nix_check`

Check a nix flake for errors with `nix flake check`.

### Parameters

- `flake` (string, required) - a flake reference, for example `path:./.` or
  `github:foo/bar`.
- `all_systems` (boolean, optional) - check the flake's outputs for all systems,
  not just the current one (passes `--all-systems`).
- `no_build` (boolean, optional) - only check that the flake evaluates, without
  building any derivations (passes `--no-build`).
- `show_trace` (boolean, optional) - pass `--show-trace` so that evaluation
  errors include the full stack trace.

### Result

On success the tool returns a text result containing the standard output of
`nix flake check`.

On failure the tool returns a text result with `isError` set and the
`nix flake check` error output as its content.

### Examples

| Flake      | Options       | Result                           |
| ---------- | ------------- | -------------------------------- |
| `path:./.` | -             | success, no errors found         |
| `path:./.` | `no_build`    | success, only evaluation checked |
| `path:./.` | `all_systems` | success, all systems evaluated   |

## `nix_log`

Fetch the build log of a nix package or store path with `nix log`, truncated to
a page. Build logs (for example from nixos tests) can be extremely long, so the
tool returns a window of lines plus a footer describing the window and how to
fetch the next page.

### Parameters

- `package` (string, required) - a nix package reference or store path, for
  example `nixpkgs#hello` or the failing `/nix/store/...-.drv` printed by a
  build error.
- `offset` (integer, optional) - the 0-based line the page starts at, defaults
  to `0`. When `from_end` is set, the offset is counted from the end of the log.
- `limit` (integer, optional) - the number of lines per page, defaults to `100`.
- `from_end` (boolean, optional) - return a window at the end of the log instead
  of the beginning, for example to see the error at the end of a nixos test log.
  Instead of `offset..offset + limit` the page is
  `total - offset - limit..total - offset`, so `from_end` with an `offset` of
  `0` returns the last `limit` lines and increasing `offset` walks backwards
  through the log.

### Result

The page contains the requested window of log lines followed by a footer in the
form `[nix_log] lines 100..199 of 24781 · next offset: 200`. A page that covers
the end of the log ends with `· end of log` instead. When no build log is
available the tool reports that instead of failing.

### Examples

| Package         | Options                   | Result            |
| --------------- | ------------------------- | ----------------- |
| `nixpkgs#hello` | -                         | first 100 lines   |
| `.drv`          | `from_end`                | last 100 lines    |
| `.drv`          | `from_end`, `offset: 200` | ends 200 from end |

[bubblewrap]: https://github.com/containers/bubblewrap
