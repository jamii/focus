{ cross ? false }:

let

  nixpkgs = <nixpkgs>;

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
        url = "https://ziglang.org/download/0.7.1/zig-linux-x86_64-0.7.1.tar.xz";
        sha256 = "1jpp46y9989kkzavh73yyd4ch50sccqgcn4xzcflm8g96l3azl40";
    } else if (hostPkgs.system == "aarch64-linux") then {
        url = "https://ziglang.org/download/0.7.1/zig-linux-aarch64-0.7.1.tar.xz";
        sha256 = "02fvph5hvn5mrr847z8zhs35kafhw5pik6jfkx3rimjr65pqpd9v";
    } else throw ("Unknown system " ++ hostPkgs.system));
    dontConfigure = true;
    dontBuild = true;
    installPhase = ''
      mkdir -p $out
      mv ./lib $out/
      mkdir -p $out/bin
      mv ./zig $out/bin
      mkdir -p $out/doc
      #mv ./langref.html $out/doc
    '';
  };

in

hostPkgs.mkShell rec {
  buildInputs = [
    zig
    hostPkgs.pkg-config
    hostPkgs.patchelf
    targetPkgs.libGL.all
    targetPkgs.xorg.libX11.dev
    targetPkgs.xlibs.xorgproto
    targetPkgs.SDL2.all
    targetPkgs.SDL2_ttf.all
  ];
  FOCUS="nixos@192.168.1.83";
  NIX_GCC=targetPkgs.gcc;
  NIX_LIBGL_LIB=targetPkgs.libGL;
  NIX_SDL2_LIB=targetPkgs.SDL2;
  NIX_SDL2_TTF_LIB=targetPkgs.SDL2_ttf;
  NIX_LIBGL_DEV=targetPkgs.libGL.dev;
  NIX_LIBX11_DEV=targetPkgs.xorg.libX11.dev;
  NIX_XORGPROTO_DEV=targetPkgs.xlibs.xorgproto;
  NIX_SDL2_DEV=targetPkgs.SDL2.dev;
  NIX_SDL2_TTF_DEV=targetPkgs.SDL2_ttf; # no .dev
  # TODO with SDL_VIDEODRIVER=wayland, SDL doesn't seem to respect xkb settings eg caps vs ctrl
  # but without, sometimes see https://github.com/swaywm/sway/issues/5227
  # SDL_VIDEODRIVER="wayland";
}
