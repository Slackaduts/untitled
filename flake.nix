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
        rustToolchain = pkgs.rust-bin.stable."1.94.1".default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
        isLinux = pkgs.stdenv.isLinux;
        isDarwin = pkgs.stdenv.isDarwin;
      in {
        devShells.default = pkgs.mkShell rec {
          nativeBuildInputs = with pkgs; [
            rustToolchain
            pkg-config
            clang
            gh

            # Python (for tools/generate_maps.py)
            uv
            (python3.withPackages (ps: with ps; [
              pillow
              numpy
              scipy
            ]))
          ] ++ pkgs.lib.optionals isLinux [
            pkgs.mold
          ];

          buildInputs = with pkgs; [
            # Lua
            lua5_4

            # Profiling
            tracy
          ] ++ pkgs.lib.optionals isLinux [
            # Bevy Linux dependencies
            pkgs.alsa-lib
            pkgs.udev
            pkgs.vulkan-loader
            pkgs.wayland
            pkgs.libdecor
            pkgs.libxkbcommon
            pkgs.libx11
            pkgs.libxcursor
            pkgs.libxi
            pkgs.libxrandr
          ] ++ pkgs.lib.optionals isDarwin [
            # Frameworks (AudioUnit, CoreAudio, Cocoa, Metal, QuartzCore, etc.)
            # are provided by the default SDK in stdenv. Add apple-sdk_15 only if
            # you need APIs newer than the default SDK (14.4).
            pkgs.apple-sdk_15
          ];

          LD_LIBRARY_PATH = pkgs.lib.optionalString isLinux
            (pkgs.lib.makeLibraryPath buildInputs);
        };
      });
}
