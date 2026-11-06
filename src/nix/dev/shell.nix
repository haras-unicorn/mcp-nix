{ inputs, ... }:

{
  self.devShells.default = {
    flake = true;
    value =
      {
        config,
        lib,
        pkgs,
        ...
      }:
      let
        yaml = pkgs.formats.yaml { };

        toml = pkgs.formats.toml { };

        json = pkgs.formats.json { };

        rustBin = inputs.haraspkgs.inputs.rust-overlay.lib.mkRustBin { } pkgs;

        rust = rustBin.stable.latest.default.override {
          extensions = [
            "rustfmt"
            "clippy"
            "rust-analyzer"
            "rust-src"
          ];
        };

        craneLib = (inputs.haraspkgs.inputs.crane.mkLib pkgs).overrideToolchain (
          _: rust
        );

        cargoVendor = craneLib.vendorCargoDeps {
          cargoLock = ../../../Cargo.lock;
        };

        prettierConfig = yaml.generate "prettier.yaml" {
          printWidth = 80;
          tabWidth = 2;
          useTabs = false;
          trailingComma = "all";
          endOfLine = "lf";
          proseWrap = "always";
        };

        cspellRoot = yaml.generate "cspell.yaml" {
          useGitignore = true;
          version = "0.2";
          language = "en";
          ignorePaths = [
            "CHANGELOG.md"
            "Cargo.lock"
          ];
          words = [
            "aarch64"
            "anyhow"
            "bubblewrap"
            "BASHOPTS"
            "bwrap"
            "cachix"
            "chdir"
            "clearenv"
            "clippy"
            "coreutils"
            "crane"
            "cwd"
            "Diátaxis"
            "devshell"
            "devshells"
            "direnv"
            "dotenv"
            "envrc"
            "fetchGit"
            "fetchTarball"
            "fetchurl"
            "haras"
            "haraspkgs"
            "mdbook"
            "mcp-nix"
            "musl"
            "netrc"
            "nixConfig"
            "nixfmt"
            "nixos"
            "nixpkgs"
            "nsenter"
            "nsswitch"
            "oxalica"
            "pkgs"
            "PPID"
            "rmcp"
            "rustfmt"
            "sandboxed"
            "sandboxing"
            "schemars"
            "serde"
            "setenv"
            "SHELLOPTS"
            "setuid"
            "substituters"
            "tarballs"
            "taplo"
            "thiserror"
            "TMPDIR"
            "tmpfs"
            "tokio"
            "treefmt"
            "unbuildable"
            "unsandboxed"
            "userns"
            "MCP_NIX_TEST_VAR"
            "codegen"
            "cooldown"
            "stdenv"
            "langservers"
            "fieldless"
            "dtolnay"
            "ieni"
            "pname"
            "rustflags"
            "mktemp"
            "podman"
            "netavark"
            "testcontainers"
            "dockerTools"
            "subuid"
            "newuidmap"
            "newgidmap"
            "nftables"
            "udevadm"
            "unshare"
            "userxattr"
            "pg8000"
            "kvm"
            "RYUK"
            "subgid"
            "subuids"
            "seccomp"
            "netns"
            "cgroupfs"
            "graphroot"
            "runroot"
            "mountopt"
            "nodev"
            "initdb"
            "pgdata"
            "setpriv"
            "reuid"
            "regid"
            "shellcheck"
            "Popen"
            "virtualisation"
            "subtest"
            "pgrep"
            "odman"
            "getuid"
          ];
        };

        markdownlintConfig = json.generate "markdownlint.json" {
          "$schema" =
            "https://raw.githubusercontent.com/DavidAnson/markdownlint/main/schema/markdownlint-config-schema.json";
          no-duplicate-heading = false;
          line-length = false;
        };

        markdownLinkCheckConfig = json.generate "markdown-link-check.json" {
          ignorePatterns = [
            { pattern = "^#"; }
            { pattern = "^https://localhost"; }
            { pattern = "^http://localhost"; }
            { pattern = "^https://haras-unicorn.github.io/mcp-nix"; }
            { pattern = "^https://github.com/haras-unicorn/mcp-nix"; }
          ];
        };

        helixServers = lib.mapAttrs (
          _: server:
          {
            inherit (server) command;
          }
          // lib.optionalAttrs (server.arguments != [ ]) {
            args = server.arguments;
          }
          // lib.optionalAttrs (server.config != { }) {
            inherit (server) config;
          }
        ) config.languages.servers;

        helixLanguages = lib.mapAttrsToList (
          name: language:
          {
            inherit name;
            auto-format = language.auto-format;
          }
          // lib.optionalAttrs (language.servers != [ ]) {
            language-servers = language.servers;
          }
          // lib.optionalAttrs (language.formatter != null) {
            formatter = {
              inherit (language.formatter) command;
            }
            // lib.optionalAttrs (language.formatter.arguments != [ ]) {
              args = language.formatter.arguments;
            };
          }
        ) config.languages.languages;

        helixLanguagesToml = toml.generate "languages.toml" (
          lib.optionalAttrs (helixServers != { }) {
            language-server = helixServers;
          }
          // lib.optionalAttrs (helixLanguages != [ ]) {
            language = helixLanguages;
          }
        );

        nixfmtWrapped = pkgs.harasPackages.makeBubblewrapWrapper {
          name = "nixfmt";
          package = pkgs.nixfmt;
        };

        prettierWrapped = pkgs.harasPackages.makeBubblewrapWrapper {
          name = "prettier";
          package = pkgs.prettier;
        };

        taploWrapped = pkgs.harasPackages.makeBubblewrapWrapper {
          name = "taplo";
          package = pkgs.taplo;
        };

        cspellWrapped = pkgs.harasPackages.makeBubblewrapWrapper {
          name = "cspell";
          package = pkgs.cspell;
        };

        hxWrapped = pkgs.harasPackages.makeBubblewrapWrapper {
          name = "hx";
          package = pkgs.helix;
          files = {
            "~/.cache/helix" = {
              mode = "rw";
            };
            ".helix/languages.toml" = {
              mode = "ro";
              source = helixLanguagesToml;
            };
          };
        };
      in
      {
        packages = [
          rust
          nixfmtWrapped
          prettierWrapped
          taploWrapped
          cspellWrapped
          hxWrapped
          pkgs.nil
          pkgs.nix
          pkgs.bubblewrap
          pkgs.bash
          pkgs.fd
          pkgs.mdbook
          pkgs.treefmt
          pkgs.marksman
          pkgs.markdownlint-cli
          pkgs.markdown-link-check
          pkgs.yaml-language-server
          pkgs.vscode-langservers-extracted
          pkgs.release-plz
          pkgs.cargo-edit
        ];

        env = {
          # The integration tests pin their nixpkgs to the locked revision.
          MCP_NIX_NIXPKGS_REV = inputs.nixpkgs.rev;
        };

        file.".cargo/config.toml".source = "${cargoVendor}/config.toml";

        git = {
          enable = true;
          ignore = {
            templates = [
              "Nix"
              "Rust"
            ];
            patterns = ''
              # Repository

              .env
              docs/book
            '';
          };
          attributes = ''
            * text=auto eol=lf
          '';
        };

        direnv = {
          enable = true;
          extra = "dotenv_if_exists .env";
        };

        editorconfig = {
          enable = true;
          settings."*" = {
            end_of_line = "lf";
            charset = "utf-8";
            max_line_length = 80;
            indent_style = "space";
            indent_size = 2;
            tab_width = 2;
            insert_final_newline = true;
            trim_trailing_whitespace = true;
          };
        };

        bwrap.files."cspell.yaml".source = cspellRoot;

        languages = {
          servers = {
            nil.command = "nil";

            taplo = {
              command = "taplo";
              arguments = [
                "lsp"
                "stdio"
              ];
            };

            yaml-language-server = {
              command = "yaml-language-server";
              arguments = [ "--stdio" ];
            };

            marksman = {
              command = "marksman";
              arguments = [ "server" ];
            };

            vscode-json-language-server = {
              command = "vscode-json-language-server";
              arguments = [ "--stdio" ];
              config = {
                provideFormatter = true;
                format.enable = true;
                json.validate.enable = true;
              };
            };

            rust-analyzer = {
              command = "rust-analyzer";
              config = {
                inlayHints = {
                  bindingModeHints.enable = false;
                  closingBraceHints.minLines = 10;
                  closureReturnTypeHints.enable = "with_block";
                  discriminantHints.enable = "fieldless";
                  lifetimeElisionHints.enable = "skip_trivial";
                  typeHints.hideClosureInitialization = false;
                };
                procMacro.enable = true;
                cargo.features = "all";
                check.features = "all";
              };
            };
          };

          languages = {
            nix = {
              servers = [ "nil" ];
              formatter = {
                command = lib.getExe pkgs.nixfmt;
                arguments = [
                  "--width"
                  "80"
                  "--filename"
                  "%{buffer_name}"
                ];
              };
            };

            rust.servers = [ "rust-analyzer" ];

            toml.servers = [ "taplo" ];

            yaml = {
              servers = [ "yaml-language-server" ];
              formatter = {
                command = lib.getExe pkgs.prettier;
                arguments = [
                  "--config"
                  (builtins.toString prettierConfig)
                  "--parser"
                  "yaml"
                ];
              };
            };

            json = {
              servers = [ "vscode-json-language-server" ];
              formatter = {
                command = lib.getExe pkgs.prettier;
                arguments = [
                  "--config"
                  (builtins.toString prettierConfig)
                  "--parser"
                  "json"
                ];
              };
            };

            markdown = {
              servers = [ "marksman" ];
              formatter = {
                command = lib.getExe pkgs.prettier;
                arguments = [
                  "--config"
                  (builtins.toString prettierConfig)
                  "--parser"
                  "markdown"
                ];
              };
            };
          };
        };

        license = {
          enable = true;
          spdx = "MIT";
          path = "LICENSE.md";
          attrs = {
            "<year>" = "2026";
            "<copyright holders>" = "haras-unicorn";
          };
        };

        codeOfConduct.contributorCovenant = {
          enable = true;
          path = "CODE_OF_CONDUCT.md";
        };

        cli.commands = {
          run = {
            description = "Run the MCP server over stdio.";
            packages = [ rust ];
            commands = ''
              cargo build --bin mcp-nix-hook
              MCP_NIX__PRE_BUILD_HOOK="''${MCP_NIX__PRE_BUILD_HOOK:-$PWD/target/debug/mcp-nix-hook}"
              export MCP_NIX__PRE_BUILD_HOOK
              cargo run --bin mcp-nix
            '';
          };

          check-links = {
            description = "Check Markdown links.";
            packages = [
              pkgs.fd
              pkgs.markdown-link-check
            ];
            commands = ''
              fd --glob '**/*.md' . -0 | xargs -0 -r markdown-link-check \
                --config ${markdownLinkCheckConfig} \
                --quiet
            '';
          };
        };

        workflows = {
          check = {
            on = {
              pull_request = { };
              push.branches = [ "main" ];
            };

            matrix.os = [ "ubuntu-latest" ];

            installable = null;

            steps = [
              {
                # The VM test of mcp-nix-podman runs in `nix flake check` and is
                # only fast with KVM, which the runner has but does not let
                # the build user open.
                run = ''
                  echo 'KERNEL=="kvm", GROUP="kvm", MODE="0666", OPTIONS+="static_node=kvm"' | sudo tee /etc/udev/rules.d/99-kvm4all.rules
                  sudo udevadm control --reload-rules
                  sudo udevadm trigger --name-match=kvm
                '';
              }
              {
                # Bubblewrap needs unprivileged user namespaces, which
                # Ubuntu 24.04 restricts with AppArmor. The sandboxed tests
                # skip in `nix flake check`, where bubblewrap cannot run in
                # the nix build sandbox, and run in the dev shell.
                run = "sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0";
              }
              { run = "nix flake check --show-trace"; }
              { run = "nix develop --command dev check"; }
              { run = "nix build --show-trace .#docs"; }
            ];
          };

          docs = {
            on.push.branches = [ "main" ];

            permissions = {
              contents = "read";
              "id-token" = "write";
              pages = "write";
            };

            installable = ".#docs";

            steps = [
              {
                uses = "actions/upload-pages-artifact@v3";
                "with".path = "./result";
              }
              { uses = "actions/deploy-pages@v4"; }
            ];
          };

          release-plz = {
            on.push.branches = [ "main" ];

            permissions = {
              contents = "write";
              "pull-requests" = "write";
            };

            installable = null;
            installNix = false;
            cachix = null;

            steps = [
              {
                uses = "actions/checkout@v4";
                "with"."fetch-depth" = 0;
              }
              { uses = "dtolnay/rust-toolchain@stable"; }
              {
                uses = "MarcoIeni/release-plz-action@v0.5";
                env = {
                  GITHUB_TOKEN = "\${{ secrets.RELEASE_PLZ_TOKEN }}";
                  CARGO_REGISTRY_TOKEN = "\${{ secrets.CARGO_REGISTRY_TOKEN }}";
                };
              }
            ];
          };

          release = {
            on.push.tags = [ "v[0-9]+.*" ];

            permissions.contents = "write";

            matrix.os = [
              "ubuntu-latest"
              "ubuntu-24.04-arm"
            ];

            installable = ".#mcp-nix-unwrapped";

            steps = [
              {
                env.GH_TOKEN = "\${{ secrets.GITHUB_TOKEN }}";
                run = ''
                  arch="$(uname -m)-linux"
                  mkdir -p release
                  cp -L result/bin/mcp-nix "release/mcp-nix-$arch"
                  cp -L result/bin/mcp-nix-hook "release/mcp-nix-hook-$arch"
                  tar -czf "mcp-nix-$arch.tar.gz" -C release "mcp-nix-$arch" "mcp-nix-hook-$arch"
                  gh release upload "$GITHUB_REF_NAME" "mcp-nix-$arch.tar.gz" --clobber
                '';
              }
            ];
          };
        };

        check.checks = {
          cspell = {
            packages = [ cspellWrapped ];
            commands = "cspell lint . --no-progress";
          };

          markdownlint = {
            packages = [ pkgs.markdownlint-cli ];
            commands = ''
              markdownlint --config ${markdownlintConfig} --ignore-path .gitignore --ignore CHANGELOG.md --ignore LICENSE.md .
            '';
          };

          nil = {
            packages = [
              pkgs.fd
              pkgs.nil
            ];
            commands = ''
              status=0
              output="$(fd --glob '*.nix' . -0 | xargs -0 -r nil diagnostics 2>&1)" || status=$?
              if [ -n "$output" ]; then
                printf '%s\n' "$output" >&2
                status=1
              fi
              test "$status" -eq 0
            '';
          };

          clippy = {
            packages = [
              rust
              pkgs.nix
              pkgs.bubblewrap
              pkgs.bash
            ];
            commands = ''
              MCP_NIX_NIXPKGS_REV="${inputs.nixpkgs.rev}"
              export MCP_NIX_NIXPKGS_REV
              CARGO_HOME="$(mktemp -d)"
              export CARGO_HOME
              trap 'rm -rf "$CARGO_HOME"' EXIT
              cp ${cargoVendor}/config.toml "$CARGO_HOME/config.toml"
              cargo clippy --all-targets -- -D warnings
            '';
          };

          # The tests drive a real nix. A build sandbox has no nix daemon and no
          # experimental features, so there they build in a store of their own
          # that lives in the temporary directory. The store is opened once
          # before the tests start, because tests that open a fresh store at the
          # same time fail with a busy database.
          test = {
            packages = [
              rust
              pkgs.nix
              pkgs.bubblewrap
              pkgs.bash
            ];
            commands = ''
              MCP_NIX_NIXPKGS_REV="${inputs.nixpkgs.rev}"
              export MCP_NIX_NIXPKGS_REV
              CARGO_HOME="$(mktemp -d)"
              export CARGO_HOME
              trap 'rm -rf "$CARGO_HOME"' EXIT
              cp ${cargoVendor}/config.toml "$CARGO_HOME/config.toml"
              if [ -n "''${NIX_BUILD_TOP:-}" ]; then
                XDG_CACHE_HOME="$TMPDIR/cache"
                export XDG_CACHE_HOME
                NIX_REMOTE="local?root=$TMPDIR/store"
                export NIX_REMOTE
                NIX_CONFIG="$(printf '%s\n' \
                  'experimental-features = nix-command flakes' \
                  'sandbox = false' \
                  'build-users-group =' \
                  'substitute = false')"
                export NIX_CONFIG
                nix store info
              fi
              cargo test --profile ci
            '';
          };
        };

        format = {
          excludes = [
            "target/**"
            ".cargo/**"
          ];

          formatters = {
            nixfmt = {
              packages = [ nixfmtWrapped ];
              command = "nixfmt";
              options = [
                "--width"
                "80"
              ];
              includes = [ "*.nix" ];
            };

            rustfmt = {
              packages = [ rust ];
              command = "rustfmt";
              options = [
                "--edition"
                "2024"
              ];
              includes = [ "*.rs" ];
            };

            taplo = {
              packages = [ taploWrapped ];
              command = "taplo";
              options = [ "format" ];
              includes = [ "*.toml" ];
            };

            prettier = {
              packages = [ prettierWrapped ];
              command = "prettier";
              options = [
                "--write"
                "--config"
                (builtins.toString prettierConfig)
              ];
              includes = [
                "*.json"
                "*.md"
                "*.yaml"
                "*.yml"
              ];
              excludes = [
                "flake.lock"
                "CHANGELOG.md"
                "LICENSE.md"
              ];
            };
          };
        };
      };
  };
}
