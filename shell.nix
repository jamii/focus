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
            url = "https://ziglang.org/builds/zig-linux-x86_64-0.9.0-dev.1625+d3a099c14.tar.xz";
            sha256 = "1azv16jkn7mh1jm9m6k7d5nkjjs26dhy26azg9xcb4hg5qrznnc0";
        } else if (hostPkgs.system == "aarch64-linux") then {
        url = "https://ziglang.org/builds/zig-linux-aarch64-0.9.0-dev.1801+a4aff36fb.tar.xz";
        sha256 = "1sbkci9rs8yjvbbl6szy3hz1ihkjvcb41w6hnzlkf3p1zhc7y43i";
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
    targetPkgs.pcre2.all
  ];
  FOCUS="nixos@192.168.1.83";
  NIX_GCC=targetPkgs.gcc;
  NIX_LIBGL_DEV=targetPkgs.libGL.dev;
  NIX_LIBX11_DEV=targetPkgs.xorg.libX11.dev;
  NIX_XORGPROTO_DEV=targetPkgs.xlibs.xorgproto;
  NIX_SDL2_DEV=targetPkgs.SDL2.dev;
  NIX_SDL2_TTF_DEV=targetPkgs.SDL2_ttf; # no .dev
  # TODO with SDL_VIDEODRIVER=wayland, SDL doesn't seem to respect xkb settings eg caps vs ctrl
  # but without, sometimes causes https://github.com/swaywm/sway/issues/5227
  # SDL_VIDEODRIVER="wayland";
  NIX_PCRE2_DEV = targetPkgs.pcre2.dev;
}
