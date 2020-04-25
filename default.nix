{ cross ? false }:

let

  nixpkgs = builtins.fetchTarball {
    name = "nixos-20.03";
    url = "https://github.com/NixOS/nixpkgs/archive/20.03.tar.gz";
    sha256 = "0182ys095dfx02vl2a20j1hz92dx3mfgz2a6fhn31bqlp1wa8hlq";
  };

  hostPkgs = import nixpkgs {};

  armPkgs = import nixpkgs {
    system = "aarch64-linux";
  };

  crossPkgs = import nixpkgs {
    overlays = [(self: super: {
      inherit (armPkgs)
        gcc
        mesa
        libGL
        SDL2
      ;
    })];
    crossSystem = hostPkgs.lib.systems.examples.aarch64-multiplatform;
  };

  targetPkgs = if cross then crossPkgs else hostPkgs;

in

hostPkgs.stdenv.mkDerivation rec {
  name = "memory";
  buildInputs = [
    hostPkgs.pkg-config
    hostPkgs.patchelf
    targetPkgs.libGL.all
    targetPkgs.SDL2.all
    # zig # tracking master instead
  ];
  FOCUS="nixos@192.168.1.83";
  NIX_GCC=targetPkgs.gcc;
  NIX_LIBGL_LIB=targetPkgs.libGL;
  NIX_SDL2_LIB=targetPkgs.SDL2;
  NIX_LIBGL_DEV=targetPkgs.libGL.dev;
  NIX_SDL2_DEV=targetPkgs.SDL2.dev;
}
