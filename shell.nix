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
        SDL_ttf
      ;
    })];
    crossSystem = hostPkgs.lib.systems.examples.aarch64-multiplatform;
  };

  targetPkgs = if cross then crossPkgs else hostPkgs;

  zig = hostPkgs.stdenv.mkDerivation {
    name = "zig";
    src = fetchTarball {
      url = "https://ziglang.org/builds/zig-linux-x86_64-0.7.0+f0b1b74d2.tar.xz";
      sha256 = "0pgzz6jnas9klrcc3kw0sf58cd7bi2r281crp7gymjp6ywmvcqg2";
    };
    dontConfigure = true;
    dontBuild = true;
    installPhase = ''
      mkdir -p $out
      mv ./lib $out/
      mkdir -p $out/bin
      mv ./zig $out/bin
      mkdir -p $out/doc
      mv ./langref.html $out/doc
    '';
  };

in

hostPkgs.mkShell rec {
  buildInputs = [
    zig
    hostPkgs.pkg-config
    hostPkgs.patchelf
    targetPkgs.libGL.all
    targetPkgs.SDL2.all
    targetPkgs.SDL2_ttf.all
  ];
  FOCUS="nixos@192.168.1.83";
  NIX_GCC=targetPkgs.gcc;
  NIX_LIBGL_LIB=targetPkgs.libGL;
  NIX_SDL2_LIB=targetPkgs.SDL2;
  NIX_SDL2_TTF_LIB=targetPkgs.SDL2_ttf;
  NIX_LIBGL_DEV=targetPkgs.libGL.dev;
  NIX_SDL2_DEV=targetPkgs.SDL2.dev;
  NIX_SDL2_TTF_DEV=targetPkgs.SDL2_ttf; # no .dev
  # TODO with SDL_VIDEODRIVER=wayland, SDL doesn't seem to respect xkb settings eg caps vs ctrl
  # but without, sometimes see https://github.com/swaywm/sway/issues/5227
  # SDL_VIDEODRIVER="wayland";
}
