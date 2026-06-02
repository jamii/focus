{ pkgs ? import <nixpkgs> {} }:

let
  # The `cargo hfuzz` subcommand isn't packaged in nixpkgs (only the C
  # honggfuzz tool is), so build it from the honggfuzz crate. This replaces
  # `cargo install honggfuzz`. Bump `version` in step with the honggfuzz
  # dependency in Cargo.lock; refresh both hashes when you do (nix prints the
  # expected values on mismatch).
  cargo-hfuzz = pkgs.rustPlatform.buildRustPackage rec {
    pname = "cargo-hfuzz";
    version = "0.5.60";
    src = pkgs.fetchCrate {
      pname = "honggfuzz";
      inherit version;
      sha256 = "sha256-btHYe+rN28bVeDWZB3AQCeF5mk30YNIINMXOOoTIjJk=";
    };
    cargoHash = "sha256-9jlu9PDqQRW3r+ZJrGxDXB533gTa8XexZuK5LXcNY3s=";
    doCheck = false;
  };
in
pkgs.mkShell {
  # honggfuzz's libhfuzz redefines libc symbols (strcpy, etc.) as weak
  # aliases. Nix's cc-wrapper auto-enables fortify, which makes glibc's
  # headers declare those same names as __fortify_clang_overload_arg,
  # producing redeclaration errors. Disable fortify in this shell.
  hardeningDisable = [ "fortify" "fortify3" ];

  buildInputs = [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.libGL
    # Mesa supplies the actual EGL/GL driver (llvmpipe for software
    # rendering); libGL alone is just libglvnd, the dispatcher.
    pkgs.mesa
    # honggfuzz's libhfuzz build needs bfd.h (binutils) and libunwind.
    pkgs.binutils-unwrapped
    pkgs.libunwind
    # Coverage reports for tests and fuzz corpus replay.
    pkgs.cargo-llvm-cov
    pkgs.llvm
    # The `cargo hfuzz` subcommand, built above (avoids `cargo install`).
    cargo-hfuzz
  ];

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.libGL
    pkgs.mesa
  ];

  # Tell libglvnd where to find Mesa's EGL vendor manifest, and where
  # Mesa's DRI driver shared objects live (llvmpipe ships as a DRI driver
  # used by the surfaceless platform).
  __EGL_VENDOR_LIBRARY_FILENAMES =
    "${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json";
  LIBGL_DRIVERS_PATH = "${pkgs.mesa}/lib/dri";
}
