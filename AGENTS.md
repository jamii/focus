It's always safe to ask questions if something doesn't make sense.

Use `./shell.nix` to install dependencies that you need.

Tests go in `tests/`. Prefer to test against an `App` with `MockIO` rather than modifying code to extract pure functions.

Don't make pointless getters/setters.

Whenever possible, every type that has non-trivial invariants should have an `assert_invariants` method that tests those invariants.

If an invariant is violated don't try to recover - panic with a useful message instead.

External I/O should only be performed through the `IO` trait, to ensure that `MockIO` is useful for testing/fuzzing.

All code in focus-core/ should be deterministic. Watch out for sneaky sources of randomness like iterating over a hashtable.