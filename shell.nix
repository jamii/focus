{ pkgs ? import <nixpkgs> {} }:

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
