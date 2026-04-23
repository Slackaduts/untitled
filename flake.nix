{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in {
        devShells.default = pkgs.mkShell rec {
          nativeBuildInputs = with pkgs; [
            rustToolchain
            pkg-config
            clang
            mold
            gh

            # Python (for tools/generate_maps.py)
            uv
            (pytho3n3.withPackages (ps: with ps; [
              pillow
              numpy
              scipy
              numpy
            ]))
          ];

          buildInputs = with pkgs; [
            # Bevy dependencies
            alsa-lib
            udev
            vulkan-loader
            wayland
            libdecor
            libxkbcommon
            libx11
            libxcursor
            libxi
            libxrandr

            # Lua
            lua5_4

            # Profiling
            tracy
          ];

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;
        };
      });
}
