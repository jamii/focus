let

  pkgs = import <nixpkgs> {};
  # crossPkgs = import <nixpkgs> {crossSystem = pkgs.lib.systems.examples.aarch64-multiplatform;};
  # hostPkgs = import <nixpkgs> {system = "aarch64-linux";};

in

pkgs.stdenv.mkDerivation rec {
  name = "memory";
  buildInputs = [
    pkgs.libGL
    pkgs.libGLU
    pkgs.glfw3
    pkgs.glew
  ];
}
