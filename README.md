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

### Packages

The flake has three packages:

| Package             | What it is                                                                                                                    |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `mcp-nix-unwrapped` | The static `mcp-nix` and `mcp-nix-hook` binaries, with nothing on their `PATH`. These are the ones in the releases.           |
| `mcp-nix`           | The default. The same binaries, wrapped so that `nix`, `bubblewrap` and `bash` are on the `PATH` and the hook is set.         |
| `mcp-nix-podman`    | `mcp-nix` plus a [container world](#containers-with-podman), so the programs can run containers. It needs a host that allows. |

### Releases

Prebuilt binaries for `x86_64-linux` and `aarch64-linux` are attached to each
[GitHub release] as tarballs containing the `mcp-nix` binary and the
`mcp-nix-hook` binary it needs, see
[Fixed-output derivations](#fixed-output-derivations). The binaries expect `nix`
and `bubblewrap` to be available on `PATH`, and `nix` on the `PATH` of the nix
daemon for the hook.

```sh
curl -L -o mcp-nix.tar.gz \
  https://github.com/haras-unicorn/mcp-nix/releases/latest/download/mcp-nix-x86_64-linux.tar.gz
tar -xzf mcp-nix.tar.gz
MCP_NIX__PRE_BUILD_HOOK="$PWD/mcp-nix-hook-x86_64-linux" \
  ./mcp-nix-x86_64-linux
```

[GitHub release]: https://github.com/haras-unicorn/mcp-nix/releases

### NixOS and home-manager

Add the flake as an input and use its package:

```nix
{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    mcp-nix.url = "github:haras-unicorn/mcp-nix";
  };

  outputs =
    { nixpkgs, mcp-nix, ... }:
    {
      nixosConfigurations.my-machine = nixpkgs.lib.nixosSystem {
        modules = [
          (
            { pkgs, ... }:
            {
              environment.systemPackages = [
                mcp-nix.packages.${pkgs.stdenv.hostPlatform.system}.default
              ];
            }
          )
        ];
      };
    };
}
```

The same package works in `home.packages` with home-manager.

### Binary cache

Builds are cached on the [haras cachix cache]. When the flake is used directly
(for example with `nix run github:haras-unicorn/mcp-nix`), the cache is
configured automatically through the flake's `nixConfig`. To use it when the
package is used from an input of your own flake, add the following to your nix
configuration:

```nix
{
  nix.settings = {
    substituters = [ "https://haras.cachix.org" ];
    trusted-public-keys = [
      "haras.cachix.org-1:/HIo1JYqOIH1Nwk1EGXhuPPvDW0WekxIbY5CiXUZbYw="
    ];
  };
}
```

[haras cachix cache]: https://app.cachix.org/cache/haras

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

## Configuration

`mcp-nix` reads its settings from, in increasing priority, the defaults, a TOML
file and the environment. The file is the path given with `--config`, which must
exist, or else `mcp-nix.toml` in the current directory, which may be missing. An
environment variable is the key in uppercase after `MCP_NIX__`, for example
`MCP_NIX__BWRAP_ARGS`.

| Key                    | Default             | Description                                                                                                             |
| ---------------------- | ------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `log_level`            | `info`              | Lowest level of the events logged as JSON on stderr.                                                                    |
| `bwrap_command`        | `bwrap`             | Command that runs bubblewrap for the programs, see [Sandboxing](#sandboxing).                                           |
| `bwrap_args`           | the default sandbox | Bubblewrap arguments of the programs, see [Sandboxing](#sandboxing).                                                    |
| `bwrap_args_extra`     | blank               | Arguments added after `bwrap_args`, see [Sandboxing](#sandboxing).                                                      |
| `nix_bwrap_command`    | `bwrap`             | Command that runs bubblewrap for `nix`, see [The sandbox of nix](#the-sandbox-of-nix).                                  |
| `nix_bwrap_args`       | the default sandbox | Bubblewrap arguments for `nix`, see [The sandbox of nix](#the-sandbox-of-nix).                                          |
| `nix_bwrap_args_extra` | blank               | Arguments added after `nix_bwrap_args`, see [The sandbox of nix](#the-sandbox-of-nix).                                  |
| `commands`             | empty, any command  | List of command file names, see [Sandboxing](#sandboxing).                                                              |
| `allow_fod`            | `false`             | Let `nix` build fixed-output derivations, see [Fixed-output derivations](#fixed-output-derivations).                    |
| `pre_build_hook`       | blank               | Absolute path of `mcp-nix-hook`, which the Nix package sets, see [Fixed-output derivations](#fixed-output-derivations). |
| `root`                 | blank               | Directory of the tools, see [Environment](#environment). Blank is the working directory of the server.                  |
| `bind_root`            | unset               | `read-only` or `read-write` binds the root into the sandbox, see [Environment](#environment).                           |
| `log_limit`            | `100`               | Lines per `nix_log` page when the caller gives no limit.                                                                |

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

The `nix` commands of the tools run in a sandbox of their own, see
[The sandbox of nix](#the-sandbox-of-nix), and the tools never build a
fixed-output derivation, see
[Fixed-output derivations](#fixed-output-derivations).

The sandbox is controlled by the `bwrap_args` setting (`MCP_NIX__BWRAP_ARGS`):

- if unset, the program runs wrapped in the default sandbox;
- if set to an empty value, the program runs directly, unsandboxed;
- otherwise, its value replaces the entire default argument set and the program
  runs as `bwrap <bwrap_args> <bwrap_args_extra> <command> <args>`.

To build on the default and not replace it, leave `bwrap_args` alone and put
what you want to add in `bwrap_args_extra` (`MCP_NIX__BWRAP_ARGS_EXTRA`), for
example a bind of one more directory. It comes after `bwrap_args`, so it can
show something through a `--tmpfs` of the default. It does nothing when the
sandbox is disabled.

The `bwrap` in that line is the `bwrap_command` setting
(`MCP_NIX__BWRAP_COMMAND`), split on whitespace. Only its first word is looked
up on the `PATH`, and the rest is passed through as is, so bubblewrap can run
inside another program, for example
`MCP_NIX__BWRAP_COMMAND="nsenter --target 1 --mount bwrap"`. The setting has no
effect when the sandbox is disabled. The sandbox of `nix` has a command of its
own, `nix_bwrap_command`.

The default sandbox and the base environment are defined in
[Sandbox configuration](#sandbox-configuration). Both default sandboxes start a
new session (`--new-session`), so a program in them cannot push input into the
terminal the server runs in.

The commands that may be executed are restricted by the `commands` setting, a
list of command file names. In the file it is a TOML array, and in the
environment (`MCP_NIX__COMMANDS`) the names are separated by commas, with no
spaces around them, for example `MCP_NIX__COMMANDS=cargo,make`. Before a
`nix_run` program or a `nix_develop` command runs, its executable file name (for
example `hello` or `cargo`) is checked against the list: when it does not match,
the tool fails with an error and nothing is executed. The check applies whenever
the list is not empty; when it is empty, any command is allowed. A variable that
is set to an empty value is a list of one empty name, which allows nothing, so
leave it unset to allow any command. A list of one name that reads as a boolean
or a number (a command called `true`) is not a list of names in the environment,
so set it in the file.

The nix store is bound read-only so that the built package, the `nix` client and
dev shell tooling are reachable inside the sandbox. Network is disabled by
default: the package is built (or the dev shell environment captured) outside
the sandbox, so the program run inside does not need network access. Set
`bwrap_args` to arguments without `--unshare-net` to allow network access. Note
that dev shell hooks that bootstrap an environment with network access (for
example `uv`) therefore require a sandbox configuration without `--unshare-net`.

To run programs unsandboxed, set `MCP_NIX__BWRAP_ARGS` to an empty string:

```json
{
  "mcpServers": {
    "mcp-nix": {
      "command": "nix",
      "args": ["run", "github:haras-unicorn/mcp-nix"],
      "env": {
        "MCP_NIX__BWRAP_ARGS": ""
      }
    }
  }
}
```

Bubblewrap requires unprivileged user namespaces, and the sandbox cannot run
inside a nix build sandbox. The `mcp-nix` package bundles both `nix` and
`bubblewrap` on its `PATH`.

Do not bind the socket of the nix daemon (`/nix/var/nix/daemon-socket`), or
`/nix` as a whole, into this sandbox. A program that can reach the daemon can
run `nix` itself, with its own options and without the pre-build hook, and build
any fixed-output derivation. Those have network access, so that is a way out of
every limit here. The default only binds `/nix/store`, and a read-only bind of
the socket would not help, because connecting to a socket does not need write
access. Running programs unsandboxed has the same effect.

### Fixed-output derivations

A fixed-output derivation is how nix fetches things. Its builder has network
access, so building one that nobody looked at could run anything. The tools
therefore make `nix` run a small program, `mcp-nix-hook`, before each build it
does itself. The program refuses fixed-output derivations, impure derivations
and derivations that ask to leave the nix sandbox, so a human has to build them
first. Whatever is already in the nix store or a binary cache is used as usual.

- The `mcp-nix` Nix package sets `pre_build_hook` to its `mcp-nix-hook`. From a
  release tarball, set `MCP_NIX__PRE_BUILD_HOOK` to the absolute path of the
  binary.
- The nix daemon only takes the hook from a trusted user, so the server checks
  that the user is trusted when it starts, and does not start otherwise. Add the
  user to `trusted-users`.
- The hook says why it refused in the log of the nix daemon.
- The hook only sees builds. The sandbox of `nix` covers the rest, see below.
- Set `allow_fod` to turn the hook off, and `pre_build_hook` is then not needed.

See [Tools](https://haras-unicorn.github.io/mcp-nix/tools.html) for what the
hook decides.

### The sandbox of nix

Nix also fetches things while it evaluates, with no build in sight: a flake
input, `builtins.fetchurl` and the like. The hook never sees those. An input
needs no hash, because nix writes it into `flake.lock` itself, and an input with
`flake = false` can be any directory, tarball or repository. The request goes to
an address the caller chose, so it is also a way to send data out.

So the `nix` commands of the tools run in a bubblewrap sandbox without network,
set by the `nix_bwrap_args` setting (`MCP_NIX__NIX_BWRAP_ARGS`) in the same way
as `bwrap_args`, with `nix_bwrap_args_extra` for additions, and run by
`nix_bwrap_command` (`MCP_NIX__NIX_BWRAP_COMMAND`) in the place of
`bwrap_command`, because the two sandboxes can need different ways in:

- if unset, `nix` runs in the default sandbox;
- if set to an empty value, `nix` runs directly;
- otherwise, its value replaces the default arguments.

The default is in [Sandbox configuration](#sandbox-configuration). It is as
small as it can be and still work:

- Evaluation can only use what is in the nix store, so a flake input that is not
  there yet fails, and a human has to fetch it first, like a fixed-output
  derivation.
- The only way out is the socket of the nix daemon. The daemon builds, with the
  pre-build hook, and downloads cached outputs. The default sets
  `NIX_REMOTE=daemon`, because otherwise `nix` finds the `/nix/var/nix` that the
  sandbox makes writable and opens an empty store of its own there. Without a
  daemon, put `--setenv NIX_REMOTE auto` in `nix_bwrap_args_extra`, and then
  `nix` works on a store of its own, which the sandbox has to bind.
- The settings and the flake registry of the system are visible. On NixOS
  `/etc/nix/nix.conf` and `registry.json` are links into `/etc/static/nix`, so
  that is bound too. The global flake registry is not available, because it is
  downloaded: the tools give `nix` a blank `flake-registry` in this sandbox. A
  short reference like `nixpkgs#hello` therefore has to be in the registry of
  the system (`nix.registry` on NixOS) or be written out in full.
- The working directory of the command is added read-only by the server. A flake
  somewhere else is not visible unless `nix_bwrap_args` binds it, and a lock
  file that needs an update cannot be written.
- The environment of the server is kept, but `HOME` is a temporary directory.
  The tools turn on `nix-command` and `flakes` themselves. Any other setting in
  a `nix.conf` of the user is not visible, only `/etc/nix` is, so put it there
  or in `NIX_CONFIG`.
- The server checks that `nix` starts in its sandbox, and does not start
  otherwise.

The security concern of the sandbox is the same as for the one of the programs.
A program that can reach the daemon with no hook builds fixed-output
derivations, so only the `nix` of the tools gets the socket, and `bwrap_args`
must not. A `nix_bwrap_args` that gives `nix` the network, or binds more than it
needs, gives up the limits above.

### What the administrator has to keep true

The server cannot see the configuration of the nix daemon, so these are on
whoever runs it. Check them on the machine, for example in the NixOS
configuration.

- The user is in `trusted-users`. The server checks this when it starts.
- The daemon has `sandbox = true` (or `relaxed`, where the hook refuses what
  leaves the sandbox). With `sandbox = false` every build has network access,
  and the hook cannot help.
- The experimental feature `recursive-nix` is off. It lets a builder start more
  builds from inside the build, and nothing shows that the hook covers them. The
  hook refuses a derivation that requires the feature, but keep it off anyway.
- `accept-flake-config` is not `true` in any nix configuration. The tools refuse
  it as a reference, but with it on, a flake that the caller wrote sets the
  settings of nix, for example the binary caches to fetch from.
- `substituters` and `trusted-public-keys` only list caches you trust, since the
  daemon downloads from them for the caller.
- `/etc/nix` has no secrets. It is visible in the sandbox of `nix`, where
  evaluation can read it and show the result to the caller. The best known case
  is `access-tokens`, a setting of `nix.conf` with tokens for hosts like GitHub,
  and a `netrc` with logins for caches is the same. Keep them in the `nix.conf`
  of the user, which is not visible, or in a file outside `/etc/nix`.
- The caller cannot write the configuration or the programs of the server. The
  server reads `mcp-nix.toml` in its working directory when it is started
  without `--config`, and environment variables win over the file, so a caller
  that can write the file there can turn off the sandboxes in the next session
  unless the environment sets them. Give `--config` a file the caller cannot
  write, or set the settings in the environment.
- The `PATH` of the server has no directory the caller can write. The server
  looks up `nix`, `bash` and `bubblewrap` there, so a program of the same name
  in such a directory would run in their place, without the sandbox.

### Environment

The `nix` commands run with the environment of the MCP server, so the user's nix
configuration (for example `NIX_CONFIG` and `NIX_PATH`) applies. The sandbox of
`nix` does not clear it.

Inside the sandbox the environment is cleared with bubblewrap (`--clearenv`) and
set to a minimal set of variables defined in
[Sandbox configuration](#sandbox-configuration). `--clearenv` is passed before
the sandbox arguments, so a `--setenv` in them is kept.

For `nix_develop` the dev shell environment captured with `nix print-dev-env` is
merged into the sandbox environment (its `PATH` is prepended), and its shell
functions and shell hook run before the command. Additional environment
variables can be provided per call through the `env` parameter.

When the sandbox is disabled (`bwrap_args` set to an empty string), the program
or command runs directly and inherits the environment of the MCP server, with
the `env` parameter (and, for `nix_develop`, the dev shell environment) applied
on top.

The tools have a root directory, the `root` setting (`MCP_NIX__ROOT`), which is
the working directory of the server when it is blank. The `nix` commands run
there, and the sandbox of `nix` shows it read-only. A `cwd` parameter has to be
inside the root, with links followed, and a relative one is taken from the root.
When a program runs unsandboxed and the call gives no `cwd`, it runs in the
root.

Inside the sandbox the `cwd` must be visible too: with the default configuration
only `/nix/store`, `/usr`, `/bin`, `/sbin`, `/lib`, `/lib64`, `/tmp`, `/proc`
and `/dev` are visible, so a `cwd` in the root fails there until the sandbox
binds the root. Set `bind_root` (`MCP_NIX__BIND_ROOT`) to `read-only` or
`read-write` and the server adds the bind to the end of the sandbox arguments,
after `bwrap_args_extra`, whether `bwrap_args` is the default or your own. It is
unset by default, because the programs can then read the root, and with
`read-write` change it. It has no effect when `bwrap_args` is blank. The sandbox
of `nix` always shows the root read-only, whatever `bind_root` is.

Choose the root with care. Everything under it is visible to evaluation, so a
root like your home directory shows the caller what is in it.

## Containers with podman

The `mcp-nix-podman` package lets the programs run containers, for example the
tests of a project that start a database with [testcontainers] from images made
with `dockerTools`. Nothing gets out of the machine through them.

On start, the wrapper makes a container world: a user and network namespace with
only a loopback interface (`unshare --user --map-root-user --net`), and in it a
rootless `podman system service`, run in a bubblewrap of its own that shows it
only its state directory. That directory is temporary and is deleted when the
server stops, together with every container. The programs then run through
`nsenter` into that namespace, with the socket of the service bound and
`DOCKER_HOST` and `CONTAINER_HOST` pointing at it. The result:

- The socket of podman is bound into the sandbox of the programs on purpose. It
  is safe only because the service lives in the world with no network, which is
  why the `bwrap_args` rule about daemon sockets still holds for the nix daemon.
- Programs and containers share that loopback, so a container is at `127.0.0.1`.
  The wrapper sets `TESTCONTAINERS_HOST_OVERRIDE=127.0.0.1` and
  `TESTCONTAINERS_RYUK_DISABLED=true`, because the state and its containers are
  deleted when the server stops, so a reaper container has nothing to add.
- The shim that runs bubblewrap enters the world first, so if the world is gone
  the programs fail and do not run with the host network.
- The user is root inside the world, but the programs are not: the shim gives
  their sandbox the user and group ids of the server (`--uid` and `--gid`), so
  programs like `nix` do not take it for a root user. It is the same user
  outside the world either way.
- `nix` is not in the world. It keeps its own sandbox and the nix daemon.

The wrapper sets `MCP_NIX__BWRAP_COMMAND` and appends to
`MCP_NIX__BWRAP_ARGS_EXTRA`, so your own settings are kept. If the world does
not start, the wrapper exits with an error and does not run the server without
containers.

The host has to allow it:

- setuid `newuidmap` and `newgidmap` (on NixOS they are in `/run/wrappers/bin`,
  which the wrapper adds to the `PATH`), and a subuid and subgid range for the
  user in `/etc/subuid` and `/etc/subgid`. This is about users and not the
  kernel: a process can only map its own user into a new user namespace, and the
  setuid helpers are what map a range, from those files. Images that have users
  of their own need the range. On NixOS a normal user gets a range by default
  (the VM test sets nothing), and `users.users.<name>.autoSubUidGidRange` asks
  for one;
- unprivileged user namespaces, as for the other sandboxes;
- a kernel with nftables support (`nf_tables`) for the container bridge. The
  `nft` program comes with the package, so `networking.nftables.enable` is not
  needed;
- the images have to be in the nix store, loaded by the programs from a
  `dockerTools` archive. A container cannot pull, because there is no network.

What this does not protect against, on purpose:

- The programs control podman, so they can start privileged containers, with no
  seccomp profile and extra capabilities. These are root only inside the world,
  which has no network and shows only its state, but a kernel bug in there is a
  kernel bug.
- They can write into the image store and run images they made themselves.
- They can run a container that binds what the sandbox of the programs shows, so
  what the sandbox shows is what a container shows.

Not planned: GPUs in containers, and a filter in front of the podman socket.

`nix flake check` runs a NixOS VM test of this package
(`src/nix/tests/mcp-nix-podman.nix`). It starts the real server, and drives it
as an MCP client would: it builds, runs containers with Postgres through
testcontainers, shows that the network is blocked from the programs and from
containers, and that a killed server leaves nothing behind.

[bubblewrap]: https://github.com/containers/bubblewrap
[testcontainers]: https://testcontainers.com

<!-- ANCHOR_END: body -->

## Documentation

The documentation is available at <https://haras-unicorn.github.io/mcp-nix/>.

## Sandbox configuration

The default sandbox arguments, those of `nix` and the base environment are
defined in
[`sandbox_defaults.rs`](./src/crates/mcp-nix/src/sandbox_defaults.rs).
