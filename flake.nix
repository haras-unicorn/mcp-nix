{
  description = "MCP server that provides nix tooling";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/release-26.05";

    flake-parts.url = "github:hercules-ci/flake-parts";
    flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";

    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";

    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-parts,
      ...
    }@inputs:
    let
      nixpkgsRev = nixpkgs.rev;

      makePackages =
        pkgs:
        let
          rust = (inputs.rust-overlay.lib.mkRustBin { } pkgs).stable.latest.default.override {
            extensions = [
              "rustfmt"
              "clippy"
              "rust-analyzer"
              "rust-src"
            ];
          };
          rustc = rust;
          cargo = rust;

          naersk' = pkgs.callPackage inputs.naersk {
            inherit rustc cargo;
          };

          unwrapped = naersk'.buildPackage (
            let
              cargoToml = builtins.fromTOML (builtins.readFile "${self}/src/mcp-nix/Cargo.toml");
            in
            {
              src = pkgs.cleanSourceWith {
                src = self;
                filter =
                  path: type:
                  (pkgs.hasSuffix ".rs" path)
                  || (pkgs.hasSuffix ".toml" path)
                  || (pkgs.hasSuffix ".lock" path)
                  || (type == "directory");
              };
              MCP_NIX_NIXPKGS_REV = nixpkgsRev;
              cargoBuildOptions =
                prev:
                prev
                ++ [
                  "-p"
                  "mcp-nix"
                ];
              name = cargoToml.package.name;
              version = cargoToml.package.version;
              meta.mainProgram = "mcp-nix";
            }
          );
        in
        {
          inherit rust unwrapped;
          package =
            pkgs.callPackage
              (
                {
                  lib,
                  makeWrapper,
                  symlinkJoin,
                  mcp-nix-unwrapped,
                  nix,
                  bubblewrap,
                }:
                symlinkJoin {
                  name = "mcp-nix";
                  paths = [ unwrapped ];
                  buildInputs = [ makeWrapper ];
                  meta.mainProgram = "mcp-nix";
                  postBuild = ''
                    wrapProgram $out/bin/mcp-nix \
                      --prefix PATH : ${
                        lib.makeBinPath [
                          nix
                          bubblewrap
                        ]
                      }
                  '';
                }
              )
              {
                mcp-nix-unwrapped = unwrapped;
              };
        };
    in
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      flake.overlays =
        let
          overlay =
            final: prev:
            let
              packages = makePackages final;
            in
            {
              mcp-nix = packages.package;
              mcp-nix-unwrapped = packages.unwrapped;
            };
        in
        {
          default = overlay;
          mcp-nix = overlay;
        };

      perSystem =
        { pkgs, lib, ... }:
        let
          flake-root = pkgs.writeShellApplication {
            name = "flake-root";
            text = ''
              current="$PWD"
              while [[ "$current" != "/" ]]; do
                if [[ -f "$current/flake.nix" ]]; then
                  echo "$current"
                  exit 0
                fi
                current="$(dirname "$current")"
              done
              echo "no flake.nix found" >&2
              exit 1
            '';
          };

          external = with pkgs; [
            flake-root
            git
            nushell
            nix
            bubblewrap
            nil
            nixfmt
            markdownlint-cli
            marksman
            mdbook
            taplo
            fd
            delta
            cachix
            release-plz
            markdown-link-check
            cspell
            prettier
            vscode-langservers-extracted
            yaml-language-server
            cargo-edit
          ];

          devScriptText = pkgs.writeText "mcp-nix-dev.nu" ''
            def "main" [] {
              dev -h
            }

            def "main run" [] {
              cd (flake-root)
              cargo run --bin mcp-nix
            }

            def "main format" [] {
              cd (flake-root)
              prettier --write .
              nixfmt ...(fd '.*\.nix$' . | lines)
              cargo fmt --all
              cargo clippy --fix --allow-dirty
            }

            def "main test" [] {
              if ($env.NIX_BUILD_TOP? | is-empty) {
                cargo clippy -- -D warnings
                cargo test
              }
            }

            def "main lint" [] {
              cd (flake-root)
              prettier --check .
              cspell lint . --no-progress
              nixfmt --check ...(fd '.*\.nix$' . | lines)
              markdownlint --ignore-path .markdownignore .
              if ($env.NIX_BUILD_TOP? | is-empty) {
                (markdown-link-check
                  --config .markdown-link-check.json
                  --quiet
                  ...(fd '.*.md' . | lines))
                (taplo lint
                  --schema "https://raw.githubusercontent.com/release-plz/release-plz/refs/tags/release-plz-v0.3.148/.schema/latest.json"
                  .release-plz.toml)
                cargo clippy -- -D warnings
                cargo test
              }
            }
          '';

          devScript =
            let
              packages = makePackages pkgs;
            in
            pkgs.writeShellApplication {
              name = "dev";
              runtimeInputs = external ++ [ packages.rust ];
              text = ''nu ${devScriptText} "$@"'';
            };
        in
        {
          devShells =
            let
              packages = makePackages pkgs;
            in
            {
              default = pkgs.mkShell {
                packages = external ++ [
                  packages.rust
                  devScript
                ];
                MCP_NIX_NIXPKGS_REV = nixpkgsRev;
              };
            };

          apps =
            let
              packages = makePackages pkgs;

              app = {
                type = "app";
                program = pkgs.getExe packages.package;
                meta.description = "MCP server that provides nix tooling";
              };
              unwrappedApp = {
                type = "app";
                program = pkgs.getExe packages.unwrapped;
                meta.description = "MCP server that provides nix tooling (unwrapped)";
              };
            in
            {
              default = app;
              mcp-nix = app;
              unwrapped = unwrappedApp;
              mcp-nix-unwrapped = unwrappedApp;
            };

          packages =
            let
              packages = makePackages pkgs;

              docs =
                pkgs.runCommand "mcp-nix-docs"
                  {
                    src = self;
                    nativeBuildInputs = [ pkgs.mdbook ];
                  }
                  ''
                    mdbook build -d "$out" "$src/docs"
                  '';
            in
            {
              inherit docs;
              default = packages.package;
              mcp-nix = packages.package;
              unwrapped = packages.unwrapped;
              mcp-nix-unwrapped = packages.unwrapped;
            };
        };
    };

  nixConfig = {
    extra-substituters = [
      "https://haras.cachix.org"
    ];
    extra-trusted-public-keys = [
      "haras.cachix.org-1:/HIo1JYqOIH1Nwk1EGXhuPPvDW0WekxIbY5CiXUZbYw="
    ];
  };
}
