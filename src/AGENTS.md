Rules:

- Don't make pointless getters/setters.
- Use `cargo fmt` after making changes to rust code.

Tests go in `tests/`. Prefer to test against an `App` with `MockIO` rather than
modifying code to extract pure functions.
