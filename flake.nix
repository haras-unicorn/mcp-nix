{
  description = "MCP server that provides nix tooling";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";

    haraspkgs.url = "github:haras-unicorn/haraspkgs/v1.3.0";
    haraspkgs.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    { haraspkgs, ... }@inputs:
    haraspkgs.lib.makeFlake {
      inherit inputs;
      modules = [ ./src/nix ];
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
