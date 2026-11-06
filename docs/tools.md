# Tools

The `mcp-nix` server exposes the following tools.

## Fixed-output derivations

A fixed-output derivation is how nix fetches things, for example with `fetchurl`
or `fetchgit`. Its builder has network access, and nix compares the hash only
after the builder ran. A flake could use one to run a script with network access
before anyone looked at it, and the hash would not help.

So the tools never build one. Every `nix` command of the tools gets the
`pre-build-hook` setting, which makes nix run `mcp-nix-hook` before each build
it does itself. The hook runs `nix derivation show` on the derivation and
refuses it when it:

- is fixed-output, which is when an output has a `hash`;
- is impure (`__impure`);
- asks to build outside the nix sandbox (a set `__noChroot`, also with
  structured attributes);
- requires the system feature `recursive-nix`, which lets the builder start
  builds of its own.

The hook also refuses a derivation it cannot read, so when in doubt it says no.
When the hook refuses, nix does not build and the tool fails, naming the hook.
The reason is in the log of the nix daemon. A human has to build the derivation
first (for example with `nix build`), and after that it is in the store and used
as usual. A derivation that nix can fetch from a binary cache never gets to the
hook either. Every other derivation is built as before.

Because nix runs the hook for every build, it covers `nix_build`, `nix_run`,
`nix_develop`, `nix_check` and `nix_log`, and also what evaluation builds on its
way (import from derivation). A derivation with several outputs, like one with a
`doc` output, is never fixed-output, because nix does not allow that.

Three things have to hold for this to work. The server checks the first two when
it starts:

- The hook is an executable file at an absolute path (`pre_build_hook`). The Nix
  package sets it. The hook needs `nix` on the `PATH` of the nix daemon.
- The user is trusted by the nix daemon. The daemon ignores the setting from
  anyone else. The server asks `nix store info` and does not start if the answer
  is not a clear yes.
- Builds happen on this machine, because a remote builder never runs the hook.
  The tools pass `--builders ''` to every command.

With the nix sandbox on, nix refuses `__noChroot` by itself before it gets to
the hook. The hook is the check when the daemon has `sandbox = relaxed`.

Setting `allow_fod` to `true` turns the hook off. Nothing is refused then, and
`pre_build_hook` is not needed.

### What stops the hook from being skipped

The hook is a setting that `nix` sends to the daemon. Anything that can run
`nix` against the daemon can send another setting, or none, and then the hook
does not run. So the programs that `nix_run` and `nix_develop` start must not
reach the daemon. The default `bwrap_args` binds only `/nix/store`, which leaves
the socket of the daemon out, and it removes the network. A `bwrap_args` that
binds the socket, or `/nix`, or no sandbox at all, undoes the hook. A read-only
bind of the socket does not help, because connecting to a socket needs no write
access.

## The sandbox of nix

The hook only sees builds. Nix also fetches while it evaluates, and the `nix`
command does that itself, not the daemon:

- `builtins.fetchurl`, `builtins.fetchTarball` and `builtins.fetchGit`;
- flake inputs. An input needs no hash, because nix writes it into `flake.lock`
  itself. An input with `flake = false` can be a directory, a tarball or a
  repository, and the flake can read its files, import them as Nix code and have
  a script from it run as the builder of a derivation.

Each of those is a request to an address the caller chose, so it also sends out
whatever the caller can put in a URL. Hashes cannot gate it, since an input
needs none, and hiding them from error messages was too weak to keep.

So the `nix` commands of the tools run in a bubblewrap sandbox without network
(`nix_bwrap_args`). Evaluation can then only use what is in the nix store, and
whatever is missing fails: a human fetches it first, just like a fixed-output
derivation. The daemon does the building, with the hook, and downloads cached
outputs, and its socket is the one thing that the sandbox lets through. The
server adds the working directory of the command, read-only.

The server runs `nix --version` in the sandbox when it starts and does not start
if that fails. The tests pin down the limit: a flake input that has to be
downloaded from a server on the loopback interface never connects.

If `nix_bwrap_args` is blank, or gives `nix` the network, this section no longer
holds. See the README for the other limits of the default.

## References and working directories

Three more inputs of the caller could undo the above, so the server checks them.

- A package or flake reference that starts with a dash is refused. It is one
  argument of a `nix` command, and nix would take `--no-sandbox` or
  `--accept-flake-config` for an option. The second one lets the flake in the
  working directory, which the caller can write, set the settings of nix, for
  example a binary cache to fetch from.
- The `command` of `nix_develop` that starts with a dash is refused. The sandbox
  runs it as `bwrap <options> <command> <args>`, and bubblewrap would take
  `--share-net` or `--bind` for an option, which gives the command the network
  or the socket of the nix daemon back. The program of `nix_run` is a path in
  the store, so it cannot be one.
- The `cwd` of `nix_run` and `nix_develop` has to be a directory inside the
  `root` of the server, which is the one it runs in unless set. Links are
  followed before the check, so neither `..` nor a link leads out of it, and a
  relative path is taken from there. The sandboxes show the working directory to
  the command, so any other directory would let the caller read whatever the
  user can.

## `nix_build`

Build a nix package and return the resulting nix store path.

### Parameters

- `package` (string, required) - a nix package reference, for example
  `nixpkgs#hello`. It must not start with a dash.
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
- `cwd` (string, optional) - the working directory for the program, inside the
  `root` of the server. Inside the sandbox it must be visible too, see
  `bind_root`.

### Result

On success the tool returns the standard output of the program.

On failure the tool returns a text result with `isError` set and the error
output as its content.

### Sandboxing

See [Sandboxing](./introduction.md) for how the program is wrapped in a
bubblewrap sandbox and how its environment is set. When the `commands` setting
is not empty, the `program` must be listed in it for the tool to run.

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
  example `cargo`. It must not start with a dash.
- `show_trace` (boolean, optional) - pass `--show-trace` to `nix print-dev-env`
  so that evaluation errors include the full stack trace.
- `args` (array of strings, optional) - arguments passed to the command.
- `env` (object of strings, optional) - additional environment variables set for
  the command. The command runs in a sandbox with the dev shell environment, so
  these supplement it.
- `cwd` (string, optional) - the working directory for the command, inside the
  `root` of the server. Inside the sandbox it must be visible too, see
  `bind_root`.

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
`--unshare-net`, see [Sandboxing](./introduction.md). When the `commands`
setting is not empty, the `command` must be listed in it for the tool to run.

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
- `limit` (integer, optional) - the number of lines per page, defaults to the
  `log_limit` setting, which is `100` unless configured.
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
