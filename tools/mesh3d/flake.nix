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
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            python3
            uv
            gcc
            cmake
            ninja
          ];

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
            pkgs.stdenv.cc.cc.lib
            pkgs.zlib
            pkgs.libGL
            pkgs.glib
            pkgs.libkrb5
            pkgs.xorg.libX11
            pkgs.xorg.libXext
            pkgs.xorg.libXrender
            pkgs.xorg.libXi
            pkgs.freetype
            pkgs.fontconfig
          ] + ":/run/opengl-driver/lib";

          CUDA_HOME = "/nix/store/1mps3cdd4jmzsxcy1nnr18riy62wslsr-cuda-merged-12.9";

          shellHook = ''
            if [ ! -d .venv ]; then
              echo "Creating venv with uv..."
              uv venv .venv --python 3.13
            fi
            source .venv/bin/activate

            echo "Hunyuan3D-2 mesh generation environment"
            echo "CUDA_HOME=$CUDA_HOME"
            echo ""
            echo "First-time setup:"
            echo "  uv pip install torch==2.6.0+cu124 torchvision==0.21.0+cu124 --index-url https://download.pytorch.org/whl/cu124"
            echo "  uv pip install git+https://github.com/Tencent-Hunyuan/Hunyuan3D-2.git"
            echo "  uv pip install trimesh rembg onnxruntime fast-simplification"
            echo "  cd _hy3d_src/hy3dgen/texgen/custom_rasterizer && uv pip install . --no-build-isolation && cd -"
            echo ""
            echo "Generate meshes:"
            echo "  python generate_meshes.py --steps 50 --faces 5000"
          '';
        };
      });
}
