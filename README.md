# Stillwatch

Protects OLED panels from burn-in on Linux Wayland. Stillwatch detects real input idle, confirms the screen is showing static content, warns you with a snooze option, and then blanks the display.

Work in progress.

## Development

Toolchains come from [asdf](https://asdf-vm.com/) (`.tool-versions`). The quality gates also need `cargo-nextest`, `cargo-llvm-cov`, `cargo-deny`, `cargo-machete`, and `jscpd` (`npm install -g jscpd`).

```sh
cargo xtask install-hooks   # pre-commit runs the fast gates (fmt, clippy, size)
cargo xtask ci              # every gate, in the same order as CI
cargo xtask ci --fast       # fmt, clippy, size only
```

Individual gates: `cargo xtask gate <fmt|clippy|size|jscpd|deny|machete|coverage|bench>`. A gate whose tool isn't installed is skipped with a message, `--strict` turns that into a failure.

- `cargo xtask check-size [--max 400]` fails on any `.rs` file over 400 lines, not counting `#[cfg(test)]` items. Put big tests in a sibling `tests.rs` via `#[cfg(test)] mod tests;`.
- `cargo xtask coverage` runs the tests under `cargo llvm-cov nextest` and requires 80% line coverage for the workspace and 90% for `stillwatch-core`. Binary `main.rs` files and `xtask` are excluded. The lcov report lands in `target/coverage/lcov.info`.
- `jscpd` uses `.jscpd.json` (50 token minimum, tests excluded, any clone fails). `cargo deny` uses `deny.toml`.

### CI

`.github/workflows/ci.yml` runs on pushes to `main` and on every PR, in an `archlinux:latest` container with the Rust version from `.tool-versions`. Both jobs call the same `cargo xtask` subcommands as above.

- **lint**: fmt, clippy, check-size, jscpd, cargo deny, cargo machete, and building the benches. Every gate runs even if an earlier one failed, so one push shows all of them.
- **test**: `cargo xtask coverage`. The lcov report is uploaded as the `lcov` artifact.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.
