{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true;
          config.cudaSupport = true;
        };
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            (python3.withPackages (ps: with ps; [
              torch
              torchvision
              transformers
              pillow
              numpy
              scipy
            ]))
          ];

          shellHook = ''
            echo "AI map generation environment ready"
            echo "Run: python ../../tools/generate_maps.py"
            echo "  or: python ../../tools/generate_maps.py --force --model large"
          '';
        };
      });
}
