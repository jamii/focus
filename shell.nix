{ cross ? false }:

let

  nixpkgs = builtins.fetchTarball {
    name = "nixos-20.09";
    url = "https://github.com/NixOS/nixpkgs/archive/20.09.tar.gz";
    sha256 = "1wg61h4gndm3vcprdcg7rc4s1v3jkm5xd7lw8r2f67w502y94gcy";
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
    src = fetchTarball (if (hostPkgs.system == "x86_64-linux") then {
        url = "https://ziglang.org/builds/zig-linux-x86_64-0.7.0+39336fd2e.tar.xz";
        sha256 = "0qxspql32jvwknp0w61c6dhzf8s47p010g05w3n72npswqqxrnaj";
    } else if (hostPkgs.system == "aarch64-linux") then {
        url = "https://ziglang.org/builds/zig-linux-aarch64-0.7.0+39336fd2e.tar.xz";
        sha256 = "00cb7bhw357d1zpdw5954z30r2v55lwyw29pwsk2hp1drf2zflm5";
    } else throw ("Unknown system " ++ hostPkgs.system));
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
