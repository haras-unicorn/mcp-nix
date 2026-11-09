{ inputs, ... }:

{
  # The GPU test is a program that runs on the machine, which `dev test-cuda`
  # starts with `nix run`, and not a flake check or a NixOS test, because none
  # of those can test the GPU:
  #
  # - CI has no GPU, so `nix flake check` only evaluates this package.
  # - A QEMU machine only gets a GPU through PCI passthrough, which takes the
  #   whole GPU away from the host for as long as the machine runs.
  # - A container node runs inside the build sandbox, and the syscall filter of
  #   nix (the filter-syscalls setting) refuses a chmod that sets the setuid or
  #   setgid bit there. So suid-sgid-wrappers.service fails in every container
  #   node, and /run/wrappers/bin stays empty: there is no su, unix_chkpwd,
  #   newuidmap or newgidmap, which the user of the server and its rootless
  #   podman need. The filter also sets no_new_privs, so a setuid program
  #   would not gain anything even if it were there. Turning filter-syscalls
  #   off would lift both, but the filter stays on, so that the build sandbox
  #   stays as strict as it is.
  #
  # It starts the real mcp-nix-podman twice and drives it as an MCP client
  # would. With the GPU settings as they are, a CUDA probe has to see the GPU in
  # the sandbox of the programs and in a container. With add_cuda_bwrap_args
  # off in a configuration file, which the wrapper can only know from the
  # effective configuration, neither of them may. Each server gets a TMPDIR of
  # its own, so that other servers on the machine cannot get in the way, and
  # after it exits, what mcp-nix-podman made there has to be gone. That
  # happens in the background, so the test waits for it.
  self.overlays.overlays.cuda = {
    flake = true;
    value =
      final: _prev:
      let
        # The control node of the NVIDIA driver, which the server looks for too.
        nvidiaDevice = "/dev/nvidiactl";

        # Where libcuda is looked for when the loader does not find it by name:
        # NixOS links the driver in /run/opengl-driver, and a CDI specification
        # puts it in the library directories of the container.
        probeScript = final.writeText "cuda-probe.py" ''
          import ctypes
          import glob
          import sys

          PATTERNS = [
              "/run/opengl-driver/lib/libcuda.so*",
              "/usr/lib*/libcuda.so*",
              "/usr/lib/*/libcuda.so*",
              "/lib*/libcuda.so*",
              "/lib/*/libcuda.so*",
          ]


          def load():
              names = ["libcuda.so.1"]
              for pattern in PATTERNS:
                  names.extend(sorted(glob.glob(pattern)))
              errors = []
              for name in names:
                  try:
                      return name, ctypes.CDLL(name)
                  except OSError as error:
                      errors.append(f"{name}: {error}")
              sys.exit("libcuda cannot be loaded: " + "; ".join(errors))


          name, cuda = load()
          status = cuda.cuInit(0)
          if status != 0:
              sys.exit(f"cuInit failed with {status} through {name}")
          count = ctypes.c_int()
          status = cuda.cuDeviceGetCount(ctypes.byref(count))
          if status != 0:
              sys.exit(f"cuDeviceGetCount failed with {status}")
          print(f"devices: {count.value} through {name}")
        '';

        probe = final.writeShellApplication {
          name = "cuda-probe";
          text = ''
            exec ${final.lib.getExe final.python3} ${probeScript}
          '';
        };

        image = final.dockerTools.buildImage {
          name = "cuda-probe";
          tag = "latest";
          copyToRoot = [ probe ];
          config.Cmd = [ (final.lib.getExe probe) ];
        };

        # The arguments are those of podman run, after the image is loaded.
        containersProbe = final.writeShellApplication {
          name = "containers-cuda-probe";
          runtimeInputs = [ final.podman ];
          text = ''
            podman --remote load --input ${image} > /dev/null
            podman --remote run --rm "$@" localhost/cuda-probe:latest
          '';
        };

        clientScript = final.writeText "mcp-nix-cuda-test.py" ''
          import json
          import os
          import re
          import subprocess
          import sys
          import tempfile
          import textwrap
          import time
          from pathlib import Path

          SERVER = "mcp-nix-podman"
          SANDBOX_PROBE = sys.argv[1]
          CONTAINERS_PROBE = sys.argv[2]
          GPU_DEVICE = "nvidia.com/gpu=all"
          STOP_TIMEOUT = 60
          CLEANUP_TIMEOUT = 60

          failures = []


          def check(name, passed, detail=""):
              print(("PASS " if passed else "FAIL ") + name, flush=True)
              if not passed:
                  print(textwrap.indent(detail, "    "), flush=True)
                  failures.append(name)


          def devices(text):
              found = re.search(r"devices: (\d+)", text)
              return int(found.group(1)) if found else 0


          def check_cleaned_up(name, scratch):
              deadline = time.monotonic() + CLEANUP_TIMEOUT
              left = list(scratch.iterdir())
              while left and time.monotonic() < deadline:
                  time.sleep(0.5)
                  left = list(scratch.iterdir())
              check(name, not left, "\n".join(sorted(map(str, left))))


          class Server:
              def __init__(self, directory, scratch, *arguments):
                  self.log = tempfile.TemporaryFile(mode="w+")
                  self.process = subprocess.Popen(
                      [SERVER, *arguments],
                      cwd=directory,
                      env={**os.environ, "TMPDIR": str(scratch)},
                      stdin=subprocess.PIPE,
                      stdout=subprocess.PIPE,
                      stderr=self.log,
                      text=True,
                  )
                  self.identifier = 0

              def __enter__(self):
                  try:
                      self.request(
                          "initialize",
                          {
                              "protocolVersion": "2025-03-26",
                              "capabilities": {},
                              "clientInfo": {"name": "test", "version": "0"},
                          },
                      )
                      self.send(
                          {"jsonrpc": "2.0", "method": "notifications/initialized"}
                      )
                  except BaseException:
                      self.close()
                      raise
                  return self

              def __exit__(self, *_):
                  self.close()

              def close(self):
                  self.process.stdin.close()
                  try:
                      self.process.wait(timeout=STOP_TIMEOUT)
                  except subprocess.TimeoutExpired:
                      self.process.kill()
                      self.process.wait()
                  self.process.stdout.close()
                  self.log.close()

              def send(self, message):
                  self.process.stdin.write(json.dumps(message) + "\n")
                  self.process.stdin.flush()

              def request(self, method, params):
                  self.identifier += 1
                  self.send(
                      {
                          "jsonrpc": "2.0",
                          "id": self.identifier,
                          "method": method,
                          "params": params,
                      }
                  )
                  while True:
                      line = self.process.stdout.readline()
                      if not line:
                          self.log.seek(0)
                          raise SystemExit(
                              "the server closed its output:\n" + self.log.read()[-3000:]
                          )
                      message = json.loads(line)
                      if message.get("id") == self.identifier:
                          return message

              def run(self, package, program, *arguments):
                  reply = self.request(
                      "tools/call",
                      {
                          "name": "nix_run",
                          "arguments": {
                              "package": package,
                              "program": program,
                              "args": list(arguments),
                          },
                      },
                  )
                  result = reply["result"]
                  text = "".join(part.get("text", "") for part in result["content"])
                  return bool(result.get("isError")), text


          with tempfile.TemporaryDirectory(ignore_cleanup_errors=True) as directory:
              work = Path(directory) / "work"
              work.mkdir()
              off = Path(directory) / "off.toml"
              off.write_text("add_cuda_bwrap_args = false\n")
              scratch_on = Path(directory) / "tmp-on"
              scratch_on.mkdir()
              scratch_off = Path(directory) / "tmp-off"
              scratch_off.mkdir()

              with Server(work, scratch_on) as server:
                  failed, text = server.run(SANDBOX_PROBE, "cuda-probe")
                  check(
                      "the sandbox of the programs sees the GPU",
                      not failed and devices(text) >= 1,
                      text,
                  )

                  failed, text = server.run(
                      CONTAINERS_PROBE, "containers-cuda-probe", "--device", GPU_DEVICE
                  )
                  check(
                      "a container that asks for the GPU sees it",
                      not failed and devices(text) >= 1,
                      text,
                  )

                  failed, text = server.run(CONTAINERS_PROBE, "containers-cuda-probe")
                  check(
                      "a container that does not ask for the GPU does not see it",
                      failed,
                      text,
                  )
              check_cleaned_up("the server leaves nothing behind", scratch_on)

              with Server(work, scratch_off, "--config", str(off)) as server:
                  failed, text = server.run(SANDBOX_PROBE, "cuda-probe")
                  check(
                      "the sandbox of the programs has no GPU when the setting is off",
                      failed,
                      text,
                  )

                  failed, text = server.run(
                      CONTAINERS_PROBE, "containers-cuda-probe", "--device", GPU_DEVICE
                  )
                  check(
                      "a container cannot get the GPU when the setting is off",
                      failed,
                      text,
                  )
              check_cleaned_up(
                  "the server leaves nothing behind when the setting is off",
                  scratch_off,
              )

          print(f"{len(failures)} failed", flush=True)
          sys.exit(1 if failures else 0)
        '';
      in
      {
        mcp-nix-cuda-test = final.writeShellApplication {
          name = "mcp-nix-cuda-test";
          runtimeInputs = [
            inputs.self.packages.${final.stdenv.hostPlatform.system}.mcp-nix-podman
          ];
          text = ''
            if [ ! -e ${nvidiaDevice} ]; then
              echo "skipping: there is no NVIDIA GPU"
              exit 0
            fi
            exec ${final.lib.getExe final.python3} ${clientScript} \
              ${probe} ${containersProbe}
          '';
        };
      };
  };
}
