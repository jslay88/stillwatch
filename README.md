# Stillwatch

Protects OLED panels from burn-in on Linux Wayland. Stillwatch detects real input idle, confirms the screen is showing static content, warns you with a snooze option, and then blanks the display.

Work in progress.

## Usage

```
stillwatch status [--json]
stillwatch snooze <DURATION>          # e.g. 45m, 1h30m
stillwatch cancel-snooze
stillwatch pause
stillwatch resume
stillwatch reload
stillwatch history [--since <DURATION>] [--json]
stillwatch probe [--interval <DURATION>]
stillwatch idle-test [--timeout <DURATION>] # alias --minutes, a bare number is minutes
stillwatch config init [--force] [PATH]
stillwatch config check [PATH]
```

Most commands are stubs until the daemon's D-Bus service lands. `stillwatch <command> --help` has details.

`stillwatch idle-test` runs the daemon's idle and gamepad sources locally, no daemon needed, and prints a timestamped line each time the combined state changes: `idle`, `active (keyboard/mouse)`, or `active (gamepad: <name>)`. You only count as idle once the compositor reports input idle and no gamepad has moved past the deadzone for the timeout (default `idle.input_idle_minutes`). Ctrl-C stops it.

```
$ stillwatch idle-test --minutes 1
Watching keyboard, mouse, and gamepads with a 1m idle timeout. Ctrl-C to stop.
2026-10-02 20:41:07  idle
2026-10-02 20:41:30  active (gamepad: Xbox Wireless Controller)
2026-10-02 20:42:30  idle
2026-10-02 20:42:51  active (keyboard/mouse)
```

The daemon takes `--config <PATH>` (default `~/.config/stillwatch/config.toml`) and `--log-level <LEVEL>`. The log level comes from `--log-level`, then `RUST_LOG`, then `info` (the config's `logging.level` will slot in before the default once the daemon loads its config). Under systemd it logs to the journal, otherwise to stderr.

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
