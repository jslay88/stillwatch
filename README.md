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

The daemon takes `--config <PATH>` (default `~/.config/stillwatch/config.toml`), `--log-level <LEVEL>`, and `--capture-check <OUTPUT>` (see below). The log level comes from `--log-level`, then `RUST_LOG`, then `info` (the config's `logging.level` will slot in before the default once the daemon loads its config). Under systemd it logs to the journal, otherwise to stderr.

## Screen capture on KDE

On KDE, Stillwatch captures the screen through KWin's `org.kde.KWin.ScreenShot2` D-Bus interface. There's no screen-sharing indicator or prompt, but KWin only answers programs it has authorized, and that's done with a `.desktop` file:

- Install [`packaging/io.github.jslay88.Stillwatch.Daemon.desktop`](packaging/io.github.jslay88.Stillwatch.Daemon.desktop) into `~/.local/share/applications/` (just you) or `/usr/share/applications/` (system wide). Any `applications/` directory under `$XDG_DATA_HOME` or `$XDG_DATA_DIRS` works.
- `Exec=` has to be the absolute path of the `stillwatchd` that runs. The shipped file says `/usr/bin/stillwatchd`; if yours lives somewhere else (`~/.cargo/bin/stillwatchd`, a build directory), edit `Exec=` to that full path. `~` and bare command names don't work.
- `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2` is the line that grants access. The file name doesn't matter, and `NoDisplay=true` keeps it out of menus.

How KWin matches it: it reads the caller's `/proc/<pid>/exe`, then looks for an installed application whose first `Exec=` word resolves (following symlinks) to exactly that path. Arguments after the path are ignored. A copy of the binary somewhere else doesn't match.

Things that trip it up:

- KWin notices new and removed `.desktop` files right away, but not edits to an existing one. After editing, remove and re-add the file, or `touch ~/.local/share/applications`.
- If the `stillwatchd` binary is replaced while it's running (an upgrade or a rebuild), KWin sees `/proc/<pid>/exe` as `... (deleted)` and refuses it. Restart the daemon.

To check it, `stillwatchd --capture-check HDMI-A-1` captures that output once and prints only its size, format, and the luma grid it was downscaled to, or the error with what to fix. The daemon also checks once at startup and logs the result. Without authorization it keeps running on input idle alone.

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

### Integration tests

Backend tests run against a real D-Bus and a real compositor, both started per test by `stillwatch-testkit`: a private `dbus-daemon`, and a headless `kwin_wayland --virtual` in a throwaway sandbox (own `XDG_RUNTIME_DIR`, home, and session bus). They never touch your desktop session, so they're safe to run inside Plasma. They need `dbus` and `kwin` installed and run with the rest under `cargo xtask coverage` / `cargo xtask ci`.

If either is missing the tests skip with a message. Set `STILLWATCH_REQUIRE_DBUS=1` and `STILLWATCH_REQUIRE_KWIN=1` to make that a failure instead (CI does).

```sh
cargo nextest run -p stillwatch-testkit -p stillwatchd   # just the crates with integration tests
STILLWATCH_REQUIRE_KWIN=1 cargo nextest run --test wayland_idle
```

### CI

`.github/workflows/ci.yml` runs on pushes to `main` and on every PR, in an `archlinux:latest` container with the Rust version from `.tool-versions`. Both jobs call the same `cargo xtask` subcommands as above.

- **lint**: fmt, clippy, check-size, jscpd, cargo deny, cargo machete, and building the benches. Every gate runs even if an earlier one failed, so one push shows all of them.
- **test**: `cargo xtask coverage`, unit and integration tests together, with `STILLWATCH_REQUIRE_DBUS=1` and `STILLWATCH_REQUIRE_KWIN=1`. The lcov report is uploaded as the `lcov` artifact.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.
