{ inputs, ... }:

{
  self.overlays.overlays.mcp-nix = {
    flake = true;
    value =
      final: _prev:
      let
        crateToml = builtins.fromTOML (
          builtins.readFile ../../crates/mcp-nix/Cargo.toml
        );

        target = "${final.stdenv.hostPlatform.parsed.cpu.name}-unknown-linux-musl";

        rustBin = inputs.haraspkgs.inputs.rust-overlay.lib.mkRustBin { } final;

        toolchain = rustBin.stable.latest.default.override {
          targets = [ target ];
        };

        craneLib = (inputs.haraspkgs.inputs.crane.mkLib final).overrideToolchain (
          _: toolchain
        );

        commonArgs = {
          pname = crateToml.package.name;
          version = crateToml.package.version;
          src = craneLib.cleanCargoSource ../../..;
          strictDeps = true;
          doCheck = false;
          cargoExtraArgs = "-p ${crateToml.package.name}";
          CARGO_BUILD_TARGET = target;
          CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        unwrapped = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            meta.mainProgram = "mcp-nix";
          }
        );

        wrapped = final.symlinkJoin {
          name = "mcp-nix";
          paths = [ unwrapped ];
          nativeBuildInputs = [ final.makeWrapper ];
          meta.mainProgram = "mcp-nix";
          postBuild = ''
            wrapProgram $out/bin/mcp-nix-hook \
              --prefix PATH : ${final.lib.makeBinPath [ final.nix ]}

            wrapProgram $out/bin/mcp-nix \
              --set-default MCP_NIX__PRE_BUILD_HOOK $out/bin/mcp-nix-hook \
              --prefix PATH : ${
                final.lib.makeBinPath [
                  final.nix
                  final.bubblewrap
                  final.bash
                ]
              }
          '';
        };

        policy = final.writeText "policy.json" (
          builtins.toJSON {
            default = [ { type = "insecureAcceptAnything"; } ];
          }
        );

        containersConf = (final.formats.toml { }).generate "containers.conf" {
          containers = {
            netns = "bridge";
            cgroups = "disabled";
          };
          network = {
            network_backend = "netavark";
            network_config_dir = "/run/networks";
          };
          engine = {
            cgroup_manager = "cgroupfs";
            events_logger = "file";
          };
        };

        # The control node of the NVIDIA driver, which tells that a GPU is
        # there. It is the node the server looks for too, and the one its CUDA
        # arguments bind.
        nvidiaDevice = "/dev/nvidiactl";

        # The service runs in a bubblewrap that shows only its own state, so
        # containers cannot mount the home directory or the project. The state
        # is a fresh temporary directory per session that is deleted on exit,
        # and only this script can delete it, because it holds files owned by
        # subuids. The socket sits in a directory of its own, because that
        # directory is what gets bound into the sandbox of the programs. The
        # overlay driver needs userxattr because the mount happens inside a user
        # namespace. The service gets a TMPDIR of its own sandbox, because the
        # one of the caller is not in it.
        #
        # With MCP_NIX_PODMAN_CUDA set the service also sees the GPU: the
        # device nodes, the driver and the CDI specification of the host, which
        # NixOS writes to /run/cdi with hardware.nvidia-container-toolkit. A
        # container then gets the GPU by asking podman for
        # nvidia.com/gpu=all. Podman looks for the specification in
        # /var/run/cdi, so /var/run links to /run as it does on the host. The
        # specification also names files of the host to mount into the
        # container, like the EGL configuration in /etc, and the runtime fails
        # when they are missing, so those are bound as well, except for the
        # store and the device nodes, which are bound already. Without
        # MCP_NIX_PODMAN_CUDA none of these are bound, so a container cannot
        # ask for the GPU either, with a CDI name or with --device.
        containerSandboxInside = final.writeShellApplication {
          name = "mcp-nix-container-sandbox-inside";
          runtimeInputs = [
            final.bubblewrap
            final.iproute2
            final.jq
            final.nftables
            final.podman
          ];
          text = ''
            state="$(mktemp -d "''${TMPDIR:-/tmp}/mcp-nix-podman.XXXXXX")"
            socket="$state/socket/podman.sock"
            mkdir -p "$state/home/.config/containers" "$state/run" "$state/socket"
            ln -sf ${policy} "$state/home/.config/containers/policy.json"
            cat > "$state/storage.conf" <<EOF
            [storage]
            driver = "overlay"
            graphroot = "$state/root"
            runroot = "$state/run/containers"

            [storage.options.overlay]
            mountopt = "nodev,userxattr"
            EOF

            gpu_args=()
            if [ "''${MCP_NIX_PODMAN_CUDA:-}" = 1 ]; then
              for node in /dev/nvidia*; do
                if [ -e "$node" ]; then
                  gpu_args+=(--dev-bind "$node" "$node")
                fi
              done
              gpu_args+=(
                --dev-bind-try /dev/dri /dev/dri
                --ro-bind-try /run/opengl-driver /run/opengl-driver
                --ro-bind-try /run/cdi /run/cdi
              )
              for spec in /run/cdi/*.json; do
                [ -e "$spec" ] || continue
                while IFS= read -r path; do
                  case "$path" in
                    /nix/store/* | /dev/*) continue ;;
                  esac
                  gpu_args+=(--ro-bind-try "$path" "$path")
                done < <(
                  jq --raw-output \
                    '[.. | objects | .hostPath? // empty] | unique | .[]' \
                    "$spec"
                )
              done
            fi

            ip link set lo up
            # A new network namespace starts with forwarding off, and the
            # bridge needs it on. It is set here because this /proc is the real
            # one, while the bubblewrap below mounts its own.
            echo 1 > /proc/sys/net/ipv4/ip_forward

            bwrap \
              --die-with-parent \
              --unshare-pid \
              --unshare-ipc \
              --unshare-uts \
              --ro-bind /nix/store /nix/store \
              --ro-bind-try /usr /usr \
              --ro-bind-try /bin /bin \
              --ro-bind-try /etc/passwd /etc/passwd \
              --ro-bind-try /etc/group /etc/group \
              --ro-bind-try /etc/nsswitch.conf /etc/nsswitch.conf \
              --proc /proc \
              --dev /dev \
              --ro-bind /sys /sys \
              --tmpfs /run \
              --tmpfs /tmp \
              --tmpfs /var/tmp \
              --symlink ../run /var/run \
              "''${gpu_args[@]}" \
              --bind "$state" "$state" \
              --setenv HOME "$state/home" \
              --setenv TMPDIR /var/tmp \
              --setenv XDG_RUNTIME_DIR "$state/run" \
              --setenv CONTAINERS_CONF ${containersConf} \
              --setenv CONTAINERS_STORAGE_CONF "$state/storage.conf" \
              --setenv NETAVARK_FW nftables \
              podman system service --time 0 "unix://$socket" &
            service=$!

            # shellcheck disable=SC2329 # called by the trap below
            cleanup() {
              podman --remote rm --all --force > /dev/null 2>&1 || true
              kill "$service" || true
              wait "$service" || true
              rm -rf "$state"
            }
            trap cleanup EXIT

            for _ in $(seq 50); do
              if [ -S "$socket" ]; then
                break
              fi
              sleep 0.1
            done

            export CONTAINER_HOST="unix://$socket"
            export DOCKER_HOST="unix://$socket"

            status=0
            "$@" || status=$?
            exit "$status"
          '';
        };

        # The command and the podman service share one user and network
        # namespace made here, with the user mapped to root and the whole
        # subuid range mapped after it, so images with their own users work.
        # The network namespace has no interface but loopback, so nothing can
        # leave the machine, and containers get a bridge inside it with their
        # ports published on its loopback. Cgroups are disabled and nothing
        # limits memory, processes or disk on purpose: the sandbox, not limits,
        # is what contains the programs. newuidmap only works as the setuid
        # wrapper, which is not part of any package, so it is found in
        # /run/wrappers/bin.
        containerSandbox = final.writeShellApplication {
          name = "mcp-nix-container-sandbox";
          runtimeInputs = [
            final.util-linux
            containerSandboxInside
          ];
          text = ''
            export PATH="$PATH:/run/wrappers/bin"
            exec unshare --user --map-root-user --map-auto --net --mount \
              mcp-nix-container-sandbox-inside "$@"
          '';
        };

        # Runs inside the container sandbox for as long as the server lives,
        # and tells where to find the service. It cleans up after itself, even
        # when the server is killed, because the sandbox it runs in does.
        containerWorld = final.writeShellApplication {
          name = "mcp-nix-container-world";
          text = ''
            info="$1"
            parent="$2"
            trap 'rm -rf "$(dirname "$info")"' EXIT
            echo "$$ $DOCKER_HOST" > "$info.tmp"
            mv "$info.tmp" "$info"
            while kill -0 "$parent" 2> /dev/null; do
              sleep 1
            done
          '';
        };

        # The server runs this in place of bubblewrap (bwrap_command). The
        # network is shared here and not in the arguments, so a server that
        # ignores the setting leaves the programs with no network instead of
        # the network of the machine. The user is root in the world, so the
        # programs are given the user and group ids of the server instead, or
        # programs like nix would take the sandbox for a root user. It is the
        # same user outside the world either way.
        bwrapInContainerWorld = final.writeShellApplication {
          name = "mcp-nix-bwrap-in-container-world";
          runtimeInputs = [
            final.bubblewrap
            final.util-linux
          ];
          text = ''
            uid="$(id -u)"
            gid="$(id -g)"
            args=()
            for arg in "$@"; do
              args+=("$arg")
              if [ "$arg" = "--unshare-all" ]; then
                args+=(--share-net --uid "$uid" --gid "$gid")
              fi
            done
            exec nsenter --target "$MCP_NIX_PODMAN_WORLD" --user --net -- \
              bwrap "''${args[@]}"
          '';
        };

        # The nix commands of the server stay outside of the world, in their
        # own sandbox, so only the programs it runs join it. The environment
        # is cleared in the sandbox of the programs, so the variables the
        # container clients read are set through bwrap_args_extra. The server
        # speaks the protocol on stdout, so nothing here may write to it.
        #
        # Containers get the GPU only when there is one and the sandbox of the
        # programs has it too. The effective configuration of the server, from
        # its own command, says so: the defaults, the file and the environment
        # together, with the CUDA arguments in bwrap_args when the server adds
        # them. A configuration that does not read fails here, as it would when
        # the server starts.
        podman = final.writeShellApplication {
          name = "mcp-nix-podman";
          runtimeInputs = [
            containerSandbox
            containerWorld
            final.jq
            wrapped
          ];
          text = ''
            effective="$(mcp-nix "$@" config --format json)"
            cuda=0
            if [ -e ${nvidiaDevice} ] \
              && jq --exit-status --arg device ${nvidiaDevice} \
                '.bwrap_args | contains($device)' \
                <<< "$effective" > /dev/null; then
              cuda=1
            fi
            MCP_NIX_PODMAN_CUDA="$cuda"
            export MCP_NIX_PODMAN_CUDA

            world="$(mktemp -d -t mcp-nix-podman-world.XXXXXX)"
            mcp-nix-container-sandbox \
              mcp-nix-container-world "$world/info" "$$" >&2 &

            for _ in $(seq 300); do
              if [ -s "$world/info" ]; then
                break
              fi
              sleep 0.1
            done

            if [ ! -s "$world/info" ]; then
              echo "the container sandbox did not start" >&2
              rm -rf "$world"
              exit 1
            fi

            read -r pid docker_host < "$world/info"
            socket_dir="$(dirname "''${docker_host#unix://}")"

            extra="--bind $socket_dir $socket_dir"
            extra="$extra --setenv DOCKER_HOST $docker_host"
            extra="$extra --setenv CONTAINER_HOST $docker_host"
            extra="$extra --setenv TESTCONTAINERS_RYUK_DISABLED true"
            extra="$extra --setenv TESTCONTAINERS_HOST_OVERRIDE 127.0.0.1"

            MCP_NIX_PODMAN_WORLD="$pid"
            MCP_NIX__BWRAP_COMMAND="${final.lib.getExe bwrapInContainerWorld}"
            MCP_NIX__BWRAP_ARGS_EXTRA="''${MCP_NIX__BWRAP_ARGS_EXTRA:-} $extra"
            export MCP_NIX_PODMAN_WORLD MCP_NIX__BWRAP_COMMAND
            export MCP_NIX__BWRAP_ARGS_EXTRA
            exec mcp-nix "$@"
          '';
        };
      in
      {
        mcp-nix = wrapped;
        mcp-nix-unwrapped = unwrapped;
        mcp-nix-podman = podman;
      };
  };

  # haraspkgs exports packages under their own names only, and `nix run` on the
  # flake without an attribute needs `default`.
  perSystem =
    { config, ... }:
    {
      packages.default = config.packages.mcp-nix;
    };
}
