{ cross ? false }:

let

  pkgs = import <nixpkgs> {};

  # crossPkgs = import <nixpkgs> {crossSystem = pkgs.lib.systems.examples.aarch64-multiplatform;};

  hostPkgs = import <nixpkgs> {
    system = "aarch64-linux";
  };

  crossPkgs = import <nixpkgs> {
    overlays = [(self: super: {
      inherit (hostPkgs)
        mesa
        libGL
        libGLU
        # glfw3
      ;
    })];
    crossSystem = pkgs.lib.systems.examples.aarch64-multiplatform;
  };

in

pkgs.stdenv.mkDerivation rec {
  name = "memory";
  buildInputs = let
    inputs = pkgs: [
      pkgs.libGL.all
      pkgs.libGLU.all
      pkgs.glfw3.all
      # zig # tracking master instead
    ];
  in
    [
      pkgs.pkg-config
    ] ++
    (inputs (if cross then crossPkgs else pkgs));
}
