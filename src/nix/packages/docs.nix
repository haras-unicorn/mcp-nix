{
  self.overlays.overlays.docs = {
    flake = true;
    value = final: _prev: {
      docs =
        final.runCommand "mcp-nix-docs"
          {
            src = ../../..;
            nativeBuildInputs = [ final.mdbook ];
          }
          ''
            mdbook build -d "$out" "$src/docs"
          '';
    };
  };
}
