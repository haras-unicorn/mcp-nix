{ inputs, ... }:

{
  # A virtual machine and not a container, because the container sandbox needs
  # the setuid newuidmap, user namespaces, overlay mounts, nftables and a real
  # nix daemon, which a test container shares with the build machine.
  #
  # The test starts the real `mcp-nix-podman` and talks to it over stdio, the
  # way a harness does, so everything between the tools and the containers is
  # covered: the build hook of a real daemon, the sandbox of nix, the sandbox of
  # the programs inside the container world and the podman service.
  self.nixosTests.mcp-nix-podman = {
    flake = true;

    value = _: {
      nodes.machine =
        { pkgs, ... }:
        let
          system = pkgs.stdenv.hostPlatform.system;

          server = inputs.self.packages.${system}.mcp-nix-podman;

          python = pkgs.python3.withPackages (packages: [
            packages.pg8000
            packages.testcontainers
          ]);

          probeImage = pkgs.dockerTools.buildImage {
            name = "container-probe";
            tag = "latest";
            copyToRoot = [ pkgs.busybox ];
            config.Cmd = [
              "/bin/echo"
              "hello from the container"
            ];
          };

          postgresId = "999";

          passwd = pkgs.writeTextDir "etc/passwd" ''
            root:x:0:0:root:/root:/bin/sh
            postgres:x:${postgresId}:${postgresId}:postgres:/data:/bin/sh
          '';

          group = pkgs.writeTextDir "etc/group" ''
            root:x:0:
            postgres:x:${postgresId}:
          '';

          postgresServer = pkgs.writeShellApplication {
            name = "postgres-server";
            runtimeInputs = [
              pkgs.coreutils
              pkgs.postgresql
            ];
            text = ''
              initdb --pgdata /data --username postgres --auth trust \
                --no-locale --encoding UTF8
              # Published ports reach the container from the address of the
              # bridge, which the entries initdb writes do not allow.
              echo "host all all all trust" >> /data/pg_hba.conf
              exec postgres -D /data \
                -c listen_addresses='*' \
                -c unix_socket_directories=/tmp
            '';
          };

          postgresEntrypoint = pkgs.writeShellApplication {
            name = "postgres-entrypoint";
            runtimeInputs = [
              pkgs.coreutils
              pkgs.util-linux
            ];
            text = ''
              mkdir /data
              chown ${postgresId}:${postgresId} /data
              exec setpriv --reuid=${postgresId} --regid=${postgresId} \
                --clear-groups ${pkgs.lib.getExe postgresServer}
            '';
          };

          postgresImage = pkgs.dockerTools.buildImage {
            name = "postgres-probe";
            tag = "latest";
            copyToRoot = [
              pkgs.dockerTools.binSh
              passwd
              group
              postgresEntrypoint
            ];
            extraCommands = "mkdir -m 1777 tmp";
            # Postgres treats SIGTERM as a smart shutdown that waits for every
            # client to disconnect, and SIGINT as a fast shutdown.
            config = {
              Cmd = [ (pkgs.lib.getExe postgresEntrypoint) ];
              StopSignal = "SIGINT";
            };
          };

          # The client connects to 127.0.0.1 and not to the name the library
          # reports, because the sandbox has no /etc/hosts to resolve
          # localhost with. Two containers run at once on the same container
          # port, which is what host networking could not do.
          postgresScript = pkgs.writeText "postgres-probe.py" ''
            import subprocess
            import sys

            import pg8000.native
            from testcontainers.core.container import DockerContainer
            from testcontainers.core.wait_strategies import LogMessageWaitStrategy

            subprocess.run(["podman", "--remote", "load", "--input", sys.argv[1]], check=True)


            def postgres():
                container = DockerContainer("localhost/postgres-probe:latest")
                container.with_exposed_ports(5432)
                container.waiting_for(
                    LogMessageWaitStrategy("database system is ready to accept connections")
                )
                return container


            def query(container):
                connection = pg8000.native.Connection(
                    user="postgres",
                    host="127.0.0.1",
                    port=int(container.get_exposed_port(5432)),
                    database="postgres",
                )
                connection.run("create table probe (id integer primary key, name text)")
                connection.run("insert into probe values (1, 'hello'), (2, 'world')")
                rows = connection.run("select id, name from probe order by id")
                connection.close()
                return rows


            with postgres() as first, postgres() as second:
                ports = {first.get_exposed_port(5432), second.get_exposed_port(5432)}
                assert len(ports) == 2, ports
                for container in (first, second):
                    rows = query(container)
                    assert rows == [[1, "hello"], [2, "world"]], rows
                print("two postgres containers answered on ports", sorted(ports))
          '';

          postgresProbe = pkgs.writeShellApplication {
            name = "postgres-probe";
            runtimeInputs = [ pkgs.podman ];
            text = ''
              exec ${pkgs.lib.getExe python} ${postgresScript} ${postgresImage}
            '';
          };

          # Whether the program can connect to a server on the machine, which
          # a program that is out of the network of the machine cannot.
          networkProbe = pkgs.writeShellApplication {
            name = "network-probe";
            runtimeInputs = [
              pkgs.bash
              pkgs.coreutils
            ];
            text = ''
              # shellcheck disable=SC2016
              if timeout 3 bash -c 'exec 3<>/dev/tcp/$1/$2' _ "$1" "$2" \
                > /dev/null 2>&1; then
                echo reachable
              else
                echo blocked
              fi
            '';
          };

          containersProbe = pkgs.writeShellApplication {
            name = "containers-probe";
            runtimeInputs = [
              pkgs.coreutils
              pkgs.podman
            ];
            text = ''
              host="$1"
              port="$2"
              home="$3"
              image=localhost/container-probe:latest

              podman --remote load --input ${probeImage} > /dev/null

              echo "driver: $(podman --remote info --format '{{.Store.GraphDriverName}}')"
              echo "greeting: $(podman --remote run --rm "$image")"
              echo "uid: $(id -u)"

              # A probe that cannot connect for any reason would say blocked.
              podman --remote run --rm "$image" /bin/sh -c 'command -v wget' \
                > /dev/null

              for network in host bridge; do
                if podman --remote run --rm --network "$network" "$image" \
                  /bin/wget -T 3 -q -O /dev/null "http://$host:$port/" \
                  > /dev/null 2>&1; then
                  echo "network $network: reachable"
                else
                  echo "network $network: blocked"
                fi
              done

              listing="$(podman --remote run --rm --volume "$home:/h" "$image" \
                /bin/ls -A /h 2>&1 || true)"
              echo "home: $listing"
            '';
          };

          probes = {
            network-probe = "${networkProbe}";
            containers-probe = "${containersProbe}";
            postgres-probe = "${postgresProbe}";
          };

          # Speaks the protocol of the server on its stdio. `session` makes the
          # calls the test is about, `kill` starts the server and kills it.
          clientScript = pkgs.writeText "mcp-nix-podman-client.py" ''
            import json
            import os
            import signal
            import subprocess
            import sys

            # The address of the machine, which a server on it listens on.
            MACHINE = "192.168.1.1"
            PORT = "8080"

            with open("/etc/mcp-nix-podman/probes.json") as file:
                probes = json.load(file)

            server = subprocess.Popen(
                [sys.argv[1]],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                text=True,
            )
            identifier = 0


            def send(message):
                server.stdin.write(json.dumps(message) + "\n")
                server.stdin.flush()


            def request(method, params):
                global identifier
                identifier += 1
                send({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params})
                while True:
                    line = server.stdout.readline()
                    if not line:
                        raise SystemExit("the server closed its output")
                    message = json.loads(line)
                    if message.get("id") == identifier:
                        return message


            def call(tool, arguments):
                reply = request("tools/call", {"name": tool, "arguments": arguments})
                result = reply["result"]
                text = "".join(part.get("text", "") for part in result["content"])
                return bool(result.get("isError")), text


            def run(probe, *arguments):
                failed, text = call(
                    "nix_run",
                    {"package": probes[probe], "program": probe, "args": list(arguments)},
                )
                assert not failed, f"{probe} failed: {text}"
                print(f"--- {probe}", flush=True)
                print(text, flush=True)
                return text


            request(
                "initialize",
                {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "test", "version": "0"},
                },
            )
            send({"jsonrpc": "2.0", "method": "notifications/initialized"})

            if sys.argv[2] == "kill":
                server.send_signal(signal.SIGKILL)
                server.wait()
                sys.exit(0)

            failed, text = call("nix_build", {"package": probes["network-probe"]})
            assert not failed and text == probes["network-probe"], text

            failed, text = call("nix_build", {"package": "probe#default"})
            assert not failed, text
            print("a flake of the system registry builds", flush=True)

            failed, text = call("nix_build", {"package": ".#default"})
            assert failed and "mcp-nix-hook" in text, (failed, text)
            print("the hook refused the fixed-output derivation", flush=True)

            for host in ("127.0.0.1", MACHINE):
                assert run("network-probe", host, PORT) == "blocked", host

            out = run("containers-probe", MACHINE, PORT, "/home/agent")
            assert "driver: overlay" in out, out
            assert "greeting: hello from the container" in out, out
            assert "uid: " + str(os.getuid()) + "\n" in out, out
            assert "network host: blocked" in out, out
            assert "network bridge: blocked" in out, out
            assert "secret-file" not in out, out

            out = run("postgres-probe")
            assert "two postgres containers answered" in out, out

            server.stdin.close()
            server.wait(timeout=60)
          '';

          client = pkgs.writeShellApplication {
            name = "mcp-nix-podman-client";
            text = ''
              exec ${pkgs.lib.getExe python} ${clientScript} "$@"
            '';
          };

          # A fixed-output derivation, which the hook has to refuse. The
          # builder never runs.
          flake = pkgs.writeText "flake.nix" ''
            {
              outputs = { self }: {
                packages.${system}.default = derivation {
                  name = "mcp-nix-podman-fixed-output";
                  system = "${system}";
                  builder = "/bin/sh";
                  args = [ "-c" "echo hello > $out" ];
                  outputHashMode = "flat";
                  outputHash = "sha256-E3MwNTPc5Nd2XQFqub3Vj6dsMkn79RP0K1IQaXNpUX4=";
                };
              };
            }
          '';

          # A flake that only the system registry knows about, like the
          # nixpkgs of a NixOS configuration.
          registryFlake = pkgs.writeTextDir "flake.nix" ''
            {
              outputs = { self }: {
                packages.${system}.default = derivation {
                  name = "mcp-nix-podman-registry";
                  system = "${system}";
                  builder = "/bin/sh";
                  args = [ "-c" "echo hello > $out" ];
                };
              };
            }
          '';
        in
        {
          virtualisation = {
            memorySize = 4096;
            cores = 2;
            diskSize = 8192;
          };

          users.users.agent = {
            isNormalUser = true;
          };

          nix.settings.trusted-users = [ "agent" ];

          nix.registry.probe.to = {
            type = "path";
            path = "${registryFlake}";
          };

          environment.systemPackages = [
            server
            client
            networkProbe
            pkgs.curl
          ];

          environment.etc."mcp-nix-podman/probes.json".text = builtins.toJSON probes;
          environment.etc."mcp-nix-podman/flake.nix".source = flake;

          systemd.services.listener = {
            wantedBy = [ "multi-user.target" ];
            serviceConfig.ExecStart =
              "${pkgs.python3}/bin/python3"
              + " -m http.server 8080"
              + " --bind 0.0.0.0"
              + " --directory /var/empty";
          };
        };

      testScript = ''
        machine.wait_for_unit("multi-user.target")
        machine.wait_for_open_port(8080)

        machine.succeed("install -d -o agent /home/agent/project")
        machine.succeed("install -o agent /etc/mcp-nix-podman/flake.nix /home/agent/project/flake.nix")
        machine.succeed("install -o agent /dev/null /home/agent/secret-file")

        client = "cd /home/agent/project && mcp-nix-podman-client mcp-nix-podman"

        with subtest("the programs can connect when they are out of the sandbox"):
            for host in ("127.0.0.1", "192.168.1.1"):
                out = machine.succeed(f"su - agent -c 'network-probe {host} 8080'")
                assert out.strip() == "reachable", out

        with subtest("the tools work, and nothing gets out"):
            out = machine.succeed(f"su - agent -c '{client} session'")
            print(out)

        with subtest("nothing is left behind"):
            machine.wait_until_fails("ls -d /tmp/mcp-nix-podman*")
            machine.wait_until_fails("pgrep -f '[p]odman'")

        with subtest("a killed server leaves nothing behind"):
            machine.succeed(f"su - agent -c '{client} kill'")
            machine.wait_until_fails("ls -d /tmp/mcp-nix-podman*")
            machine.wait_until_fails("pgrep -f '[p]odman'")
      '';
    };
  };
}
