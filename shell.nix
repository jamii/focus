{ cross ? false }:

let

hostPkgs = import <nixpkgs> {};

armPkgs = import <nixpkgs> {
    system = "aarch64-linux";
};

crossPkgs = import <nixpkgs> {
    overlays = [(self: super: {
        inherit (armPkgs)
        gcc
        mesa
        libGL
        ;
    })];
    crossSystem = hostPkgs.lib.systems.examples.aarch64-multiplatform;
};

targetPkgs = if cross then crossPkgs else hostPkgs;

zig = hostPkgs.stdenv.mkDerivation {
    name = "zig";
    src = fetchTarball (
        if (hostPkgs.system == "x86_64-linux") then {
            url = "https://ziglang.org/download/0.14.0/zig-linux-x86_64-0.14.0.tar.xz";
            sha256 = "052pfb144qaqvf8vm7ic0p6j4q2krwwx1d6cy38jy2jzkb588gw3";
        } else
        throw ("Unknown system " ++ hostPkgs.system)
    );
    dontConfigure = true;
    dontBuild = true;
    installPhase = ''
    mkdir -p $out
    mv ./* $out/
    mkdir -p $out/bin
    mv $out/zig $out/bin
    '';
};

in

hostPkgs.mkShell rec {
    buildInputs = [
        zig
        hostPkgs.git
        hostPkgs.pkg-config
        targetPkgs.libGL
        targetPkgs.wayland
        targetPkgs.libxkbcommon
    ];
    LD_LIBRARY_PATH = "${targetPkgs.lib.makeLibraryPath buildInputs}";
}
