It's always safe to ask questions if something doesn't make sense.

Use `./shell.nix` to install dependencies that you need.

Tests go in `tests/`. Prefer to test against an `App` with `MockIO` rather than
modifying code to extract pure functions.

Don't make pointless getters/setters.
