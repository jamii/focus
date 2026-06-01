Tests go in `tests/`. Prefer to test against an `App` with `MockIO` rather than
modifying code to extract pure functions.

Don't make pointless getters/setters.