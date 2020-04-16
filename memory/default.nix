{ cross ? false }:

let

  pkgs = import <nixpkgs> {};

  hostPkgs = import <nixpkgs> {
    system = "aarch64-linux";
  };

  crossPkgs = import <nixpkgs> {
    overlays = [(self: super: {
      inherit (hostPkgs)
        mesa
        libGL
        SDL2
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
      pkgs.SDL2.all
      # zig # tracking master instead
    ];
  in
    [
      pkgs.pkg-config
    ] ++
    (inputs (if cross then crossPkgs else pkgs));
}
