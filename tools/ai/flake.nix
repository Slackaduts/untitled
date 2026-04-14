{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in {
        devShells = {
          # CPU-only (fast to build, ~1-2s per sprite)
          default = pkgs.mkShell {
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
              echo "AI map generation environment (CPU) ready"
              echo "Run: python ../../tools/generate_maps.py"
            '';
          };

          # CUDA (slow to build first time, ~0.1s per sprite)
          cuda = pkgs.mkShell {
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
              echo "AI map generation environment (CUDA) ready"
              echo "Run: python ../../tools/generate_maps.py"
            '';
          };
        };
      });
}
