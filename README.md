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
stillwatch probe [--interval <DURATION>] [--count <N>] [--json] [--standalone]
stillwatch idle-test [--timeout <DURATION>] # alias --minutes, a bare number is minutes
stillwatch config init [--force] [PATH]
stillwatch config check [PATH]

stillwatch-gui                  # tray; open the window from the menu
stillwatch-gui settings          # tray, and the settings window immediately
stillwatch-gui prompt            # tray, and the prompt placeholder
```

`stillwatch <command> --help` has details. Durations use [humantime](https://docs.rs/humantime) syntax (`90s`, `45m`, `1h 30m`).

Everything except `idle-test`, `config`, and `probe --standalone` talks to `stillwatchd` over D-Bus (`io.github.jslay88.Stillwatch` on the session bus).

`stillwatch-gui` is the tray and the settings window. It talks to the same daemon. The window still opens when the daemon isn't running, and the tray icon changes until the daemon comes back (it reconnects on its own). A second `stillwatch-gui` hands off to the one already running instead of starting another tray. Quick snooze uses each `[prompt] snooze_presets_minutes` value. The Settings, Calibration, History, and Service pages are placeholders for now, and so is `prompt`.

- **`status`**: state and time in it, snooze time left, idle/locked/media, the capture backend, the last stale check per output (persistent and dark percentages, threshold and why), panel care, and config errors if the last reload failed. `--json` prints one `StatusPayload` object.
- **`snooze <DURATION>`**: the daemon checks it against the `[prompt]` snooze rules (a preset, or `custom_min_minutes` to `custom_max_minutes` with `allow_custom`) and says why if it doesn't fit.
- **`cancel-snooze`**, **`pause`**, **`resume`**: what they say.
- **`reload`**: makes the daemon reload its config file and prints the result. If the new config is invalid, the problems are printed one per line, the daemon keeps the last good config, and the exit code is 1.
- **`history`**: a table of recent decisions (time, event, state change, per-output persistent percentages with the threshold and reason, snooze/blank/answer details, and media/gamepad/lock context), oldest first. `--since 2h` limits it to the last two hours. `--json` prints one `HistoryEntry` per line.
- **`probe`**: live calibration. Shows each output's block grid (`██` persistent, `░░` changed, `··` dark, `xx` ignored) with a summary line like `HDMI-A-1: persistent 72% (dark 18%, counted 230/256), threshold 70% normal -> STALE`. The interval defaults to `stale.check_interval_seconds` from the config file; `--interval 5s` is handy while tuning (100ms minimum). Runs until Ctrl-C or `--count` samples. `--json` prints one `ProbeSample` per line. `--standalone` starts `stillwatchd --probe` (the installed binary KWin has to authorize) and renders those lines; without it the running daemon is asked over D-Bus. Only block states and percentages leave the process, never pixels.

Colors are used only when stdout is a terminal and `NO_COLOR` isn't set.

Exit codes are the same for every command:

| Code | Meaning |
| -- | -- |
| 0 | Success |
| 1 | The command failed: the daemon refused it (the message says why), the config is invalid, or something else went wrong |
| 2 | Usage error (bad flags or arguments) |
| 3 | `stillwatchd` isn't running (`systemctl --user start stillwatch`) |

`stillwatch idle-test` runs the daemon's idle and gamepad sources locally, no daemon needed, and prints a timestamped line each time the combined state changes: `idle`, `active (keyboard/mouse)`, or `active (gamepad: <name>)`. You only count as idle once the compositor reports input idle and no gamepad has moved past the deadzone for the timeout (default `idle.input_idle_minutes`). Ctrl-C stops it.

```
$ stillwatch idle-test --minutes 1
Watching keyboard, mouse, and gamepads with a 1m idle timeout. Ctrl-C to stop.
2026-10-02 20:41:07  idle
2026-10-02 20:41:30  active (gamepad: Xbox Wireless Controller)
2026-10-02 20:42:30  idle
2026-10-02 20:42:51  active (keyboard/mouse)
```

The daemon takes `--config <PATH>` (default `~/.config/stillwatch/config.toml`), `--log-level <LEVEL>`, `--capture-check <OUTPUT>` (see below), and `--probe [--interval <DURATION>] [--count <N>]` (JSON lines for `stillwatch probe --standalone`). The log level comes from `--log-level`, then `RUST_LOG`, then `info` (the config's `logging.level` will slot in before the default once the daemon loads its config). Under systemd it logs to the journal, otherwise to stderr.

## Display hooks

`action.on_blank_cmd` and `action.on_resume_cmd` run as `sh -c` after a blank and on wake. They are fire-and-forget: a 10 second timeout, failures only go to the log, and they never hold up the state machine. `action.command` (when `action.mode = "command"`) uses the same runner and does not blank.

Each hook gets:

| Variable | Example | Meaning |
| -- | -- | -- |
| `STILLWATCH_OUTPUTS` | `HDMI-A-1,DP-1` | Connector names being acted on |
| `STILLWATCH_METHOD` | `dpms` | `dpms`, `overlay`, or `ddc_standby` |
| `STILLWATCH_REASON` | `blank` | `blank`, `resume`, `command`, or `panel_care` |

```toml
[action]
on_blank_cmd = "lg-webos-cli screen-off"
on_resume_cmd = "lg-webos-cli screen-on"
```

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

## Calibrating detection

`stillwatch probe` is how you tune `stale_percent`, `persist_checks`, `luma_delta_threshold`, `ignore_dark_below`, and `ignore_regions`. It shows the block grid the detector is using, not a screenshot.

```
stillwatch probe --interval 5s
stillwatch probe --standalone --interval 5s --count 8
stillwatch probe --json
```

`--interval 5s` is faster than the default `stale.check_interval_seconds` (60s) while you watch the grid. A static desktop should flip `░░` (changed) to `██` (persistent) after `persist_checks` samples. A clock, a video tile, or anything else that moves should stay `░░`. Letterboxing goes `··` (dark; those blocks are excluded). A panel clock or a status widget you don't want to count is an `ignore_regions` rect, and shows as `xx`.

The summary line is the stale fraction the daemon will use: `persistent 72% (dark 18%, counted 230/256), threshold 70% normal -> STALE`. `media` instead of `normal` means a non-ignored MPRIS player is Playing and `media_stale_percent` is in effect.

`--standalone` is the path that actually captures when the daemon isn't running yet. It starts the installed `stillwatchd` (sibling of this `stillwatch`, same binary the `.desktop` `Exec=` has to name) with `--probe`. That process prints one `ProbeSample` JSON line per sample; this command renders them with the same grid as the D-Bus probe. `--json` prints those lines as-is. Capture has to run as `stillwatchd` or KWin refuses it.

`--json` never includes luma values or pixels. Only per-block states (`changed`, `persistent`, `dark`, `ignored`) and the percentages above.

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
