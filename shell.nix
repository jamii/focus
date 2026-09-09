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
  # Needed at runtime by the built binaries, not just at build time.
  graphicsLibs = [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.libGL
    # Mesa supplies the actual EGL/GL driver (llvmpipe for software
    # rendering); libGL alone is just libglvnd, the dispatcher.
    pkgs.mesa
  ];
in
pkgs.mkShell {
  # honggfuzz's libhfuzz redefines libc symbols (strcpy, etc.) as weak
  # aliases. Nix's cc-wrapper auto-enables fortify, which makes glibc's
  # headers declare those same names as __fortify_clang_overload_arg,
  # producing redeclaration errors. Disable fortify in this shell.
  hardeningDisable = [ "fortify" "fortify3" ];

  buildInputs = graphicsLibs ++ [
    # honggfuzz's libhfuzz build needs bfd.h (binutils) and libunwind.
    pkgs.binutils-unwrapped
    pkgs.libunwind
    # Coverage reports for tests and fuzz corpus replay.
    pkgs.cargo-llvm-cov
    pkgs.llvm
    # Formatter used by `cargo fmt`.
    pkgs.rustfmt
    # The `cargo hfuzz` subcommand, built above (avoids `cargo install`).
    cargo-hfuzz
    # A headless compositor for the daemon end-to-end test, and the
    # `swaymsg -t get_tree` it asserts against. Unwrapped, because the
    # wrapper insists on a dbus session.
    pkgs.sway-unwrapped
  ];

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath graphicsLibs;

  # winit and glutin `dlopen` libwayland-client, libxkbcommon and libEGL
  # rather than linking them, so the loader only finds them via
  # LD_LIBRARY_PATH - which exists inside this shell and nowhere else, so
  # the binaries built here would only run inside it. Bake the same paths
  # into the RUNPATH instead: a `dlopen` from the executable searches the
  # executable's RUNPATH, so `target/release/focus` then works anywhere.
  #
  # This is only the client side of GL. The actual driver still comes from
  # the system: nixpkgs' libglvnd looks for its vendor manifest in
  # /run/opengl-driver/share/glvnd/egl_vendor.d, which is where NixOS puts
  # the real one. The two variables at the bottom of this file point it at
  # the mesa above instead, for the headless tests.
  RUSTFLAGS = "-C link-arg=-Wl,-rpath,${pkgs.lib.makeLibraryPath graphicsLibs}";

  # Tell libglvnd where to find Mesa's EGL vendor manifest, and where
  # Mesa's DRI driver shared objects live (llvmpipe ships as a DRI driver
  # used by the surfaceless platform).
  __EGL_VENDOR_LIBRARY_FILENAMES =
    "${pkgs.mesa}/share/glvnd/egl_vendor.d/50_mesa.json";
  LIBGL_DRIVERS_PATH = "${pkgs.mesa}/lib/dri";
}
