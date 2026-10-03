# Stillwatch

Protects OLED panels from burn-in on Linux Wayland. Desktop idle timers never fire while a video player, a browser, or a call holds an idle inhibitor, so the panel stays on a static image. Stillwatch watches real input idle (`ext-idle-notify-v1` v2, which ignores inhibitors), confirms the picture is actually static, prompts with a snooze, and then blanks the display.

## How it decides

Each output is split into a block grid. A block counts as persistent once it has stayed unchanged for `persist_checks` captures in a row, so video and animation don't trip it. The default stale threshold is 70% of the counted blocks. That is lower than an area-only check would need, because the per-block wait already rejected motion, and a call with a few video tiles over a static screen should still blank.

Blocks darker than `ignore_dark_below` (default 16) in both the previous and the current capture are left out of the count. Black OLED pixels are off and don't wear, so letterboxing and a dark wallpaper aren't burn-in. An all-dark screen is never stale.

Pixels can't tell a windowed video you're watching from a video call left running. While a non-ignored MPRIS player is Playing, the threshold rises to `media_stale_percent` (default 90). Audio-only players (Spotify by default) don't raise it.

## Desktops and backends

KDE Plasma on KWin is the desktop this is built for. Other Wayland compositors can capture through the portal. DPMS blanking goes through `kscreen-doctor`, so that path is KWin.

| Piece | What it uses |
| -- | -- |
| Input idle | `ext-idle-notify-v1` v2. A compositor that only has v1 doesn't count. Gamepads are evdev (compositors don't treat them as input). |
| Capture | `auto`: KWin `org.kde.KWin.ScreenShot2` when it's there, otherwise xdg-desktop-portal ScreenCast over PipeWire. The portal stream only runs while you're away. |
| Blank | `dpms` (`kscreen-doctor`), `ddc_standby` (MCCS power mode, VCP 0xD6, over DDC/CI), or a black layer-shell overlay. The overlay works on any compositor with layer-shell and keeps the panel on. |
| Prompt | A notification with snooze actions. `kdialog` is the fallback, and the Custom... duration. |
| Session | logind lock and sleep, plus `org.freedesktop.ScreenSaver`. |
| Media | MPRIS `PlaybackStatus` only. |

If capture isn't available, the daemon keeps running on input idle alone.

## Install

`cargo xtask install` builds the release binaries and installs them with the systemd user unit, desktop files, and icons. It does not enable the unit, start `stillwatchd`, or copy the example hooks into the config.

```sh
cargo xtask install                  # prefix ~/.local
cargo xtask install --prefix /usr    # needs root
cargo xtask uninstall --prefix ~/.local
```

`/usr` uses `/usr/bin`, `/usr/lib/systemd/user`, and `/usr/share`. Any other prefix uses the XDG layout under that directory (`bin/`, `share/systemd/user/`, `share/applications/`, `share/icons/`), which is what systemd and KWin search for a `~/.local` install.

| Installed | Path |
| -- | -- |
| `stillwatch`, `stillwatchd`, `stillwatch-gui` | `<prefix>/bin` |
| `stillwatch.service` | `~/.local/share/systemd/user/` or `/usr/lib/systemd/user/` |
| Settings launcher | `<prefix>/share/applications/io.github.jslay88.Stillwatch.desktop` (`Exec=stillwatch-gui settings`) |
| ScreenShot2 grant | `<prefix>/share/applications/io.github.jslay88.Stillwatch.Daemon.desktop` |
| Tray autostart template | `<prefix>/share/stillwatch/io.github.jslay88.Stillwatch.Tray.desktop` |
| Icons | `<prefix>/share/icons/hicolor/scalable/` |

The daemon desktop file's `Exec=` is the absolute path of the installed `stillwatchd`. The files in the repo say `/usr/bin/stillwatchd`; install rewrites that when the prefix is not `/usr`. KWin resolves the first `Exec=` word (symlinks included) and compares it to `/proc/<pid>/exe`, so the path has to be the real binary. The unit's `ExecStart=` is the same path.

When install finishes it prints:

```
systemctl --user daemon-reload && systemctl --user enable --now stillwatch
```

Run that when you want the daemon in the Plasma session. The unit is `PartOf=`, `After=`, and `WantedBy=graphical-session.target`, so it starts and stops with the graphical session. `Restart=on-failure` and `RestartSec=5s` cover a crash. `systemctl --user reload stillwatch` runs `ExecReload=/usr/bin/kill -HUP $MAINPID`, and SIGHUP reloads the config.

Logs go to the journal (`journalctl --user -u stillwatch`). systemd sets `JOURNAL_STREAM` on the unit and `stillwatchd` logs through `tracing-journald` when that matches stderr.

There is no D-Bus activation file. A bus activation would start `stillwatchd` on the first name lookup, outside the graphical session (no Wayland, and no journal stream attached to the unit). `stillwatch` already exits 3 when the name has no owner instead of starting a daemon.

The tray does not autostart on install. Copy `io.github.jslay88.Stillwatch.Tray.desktop` from `<prefix>/share/stillwatch/` to `~/.config/autostart/` when you want the tray at login. The settings launcher is the desktop file in `share/applications`.

Icons: `io.github.jslay88.Stillwatch` is the app icon. Status icons `io.github.jslay88.Stillwatch-{down,active,monitoring,prompting,snoozed,acting,blanked,locked,paused}` are the tray states.

`cargo xtask uninstall` removes the files and prints `systemctl --user disable --now stillwatch && systemctl --user daemon-reload`. It does not run that, and it does not stop a daemon that is already up.

### Arch

[`packaging/arch/PKGBUILD`](packaging/arch/PKGBUILD) builds this checkout. `prepare` fetches crates with `cargo fetch --locked`. `build` is `cargo build --release --offline --locked`. It needs Rust 1.99 (`rust`/`cargo`, or the asdf toolchain in [`.tool-versions`](.tool-versions)).

```sh
cd packaging/arch
makepkg -si
```

The package depends on `kscreen`, `dbus`, `pipewire`, and `xdg-desktop-portal`. `ddcutil` is optional: `ddc_standby` talks DDC/CI itself, and it needs the `i2c-dev` module, which is what the `ddcutil` package loads and what you probe the bus with. `kdialog` is optional, for the dialog prompt. The package installs the binaries, the user unit, the desktop files (the ScreenShot2 grant's `Exec=` is `/usr/bin/stillwatchd`), icons, `README.md`, [`docs/config.md`](docs/config.md), the example hooks under `/usr/share/doc/stillwatch/hooks/`, and both license files. It does not enable the unit. After it installs, run the same `systemctl --user` line as above.

## First run

On KDE, capture goes through KWin's `org.kde.KWin.ScreenShot2`. There's no screen-sharing indicator, but KWin only answers programs it has authorized. That's a `.desktop` file:

- `cargo xtask install` and the PKGBUILD both install [`packaging/io.github.jslay88.Stillwatch.Daemon.desktop`](packaging/io.github.jslay88.Stillwatch.Daemon.desktop) into `share/applications/` with `Exec=` set to the installed `stillwatchd`. For a hand install, copy that file into `~/.local/share/applications/` (just you) or `/usr/share/applications/` (system wide) and edit `Exec=` if the binary is not `/usr/bin/stillwatchd`. Any `applications/` directory under `$XDG_DATA_HOME` or `$XDG_DATA_DIRS` works.
- `Exec=` has to be the absolute path of the `stillwatchd` that runs. The shipped file says `/usr/bin/stillwatchd`. `~` and bare command names don't work.
- `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2` is the line that grants access. The file name doesn't matter, and `NoDisplay=true` keeps it out of menus.

How KWin matches it: it reads the caller's `/proc/<pid>/exe`, then looks for an installed application whose first `Exec=` word resolves (following symlinks) to exactly that path. Arguments after the path are ignored. A copy of the binary somewhere else doesn't match.

Things that trip it up:

- KWin notices new and removed `.desktop` files right away, but not edits to an existing one. After editing, remove and re-add the file, or `touch ~/.local/share/applications`.
- If the `stillwatchd` binary is replaced while it's running (an upgrade or a rebuild), KWin sees `/proc/<pid>/exe` as `... (deleted)` and refuses it. Restart the daemon.

Check it with `stillwatchd --capture-check HDMI-A-1`. That captures the output once and prints its size, format, and the luma grid it was downscaled to, or the error and what to fix. The daemon also checks once at startup and logs the result.

Then:

```sh
stillwatch config init
systemctl --user daemon-reload && systemctl --user enable --now stillwatch
```

`config init` writes a commented default to `~/.config/stillwatch/config.toml`. The daemon runs on defaults if the file isn't there yet. Enabling the unit is the step that starts it. Install doesn't.

## Configuration

Every key is in [`docs/config.md`](docs/config.md). `stillwatch config check` validates a file without the daemon. The settings window edits the same file; the daemon reloads it.

Unknown keys are rejected. Most changes apply on reload. `capture.backend`, `stale.block_grid`, and `stale.monitored_outputs` rebuild capture and reset the block counters.

### Display presets

A preset writes ordinary keys. There is no `profile` field in the file.

| Setup | What to set |
| -- | -- |
| OLED monitor | `blank_method = "dpms"`, `reblank_on_wake = true`, `reblank_fallback = "overlay"`. DPMS lets the panel reach standby, so its own compensation cycle can run. If a display wakes itself when the HDMI link drops, the re-blank watchdog blanks it again, and the overlay is the fallback that keeps the signal alive. |
| OLED TV | `blank_method = "overlay"`, which leaves the TV on but covers it. Or `dpms` plus an `on_blank_cmd` / `on_resume_cmd` hook that tells the TV to turn its own screen off. The overlay blocks panel compensation because the panel stays powered. |
| Mixed OLED and LCD | Put the OLED connector names in `stale.monitored_outputs` and set `outputs = "monitored"`. LCDs are left out of detection and out of the blank. |
| Custom | Leave the keys alone. |

### Display hooks

`action.on_blank_cmd` and `action.on_resume_cmd` run as `sh -c` after a blank and on wake. They are fire-and-forget: a 10 second timeout, failures only go to the log, and they never hold up the state machine. `action.command` (when `action.mode = "command"`) uses the same runner and does not blank. `panel_care.trigger_cmd` uses it at blank time when panel care is due.

The examples in [`packaging/hooks/`](packaging/hooks/) are not installed as active hooks. `cargo xtask install` doesn't copy them. The PKGBUILD puts them in `/usr/share/doc/stillwatch/hooks/` and still doesn't point the config at them. The path in the config has to be absolute.

Each script documents the environment Stillwatch sets:

| Variable | Example | Meaning |
| -- | -- | -- |
| `STILLWATCH_OUTPUTS` | `HDMI-A-1,DP-1` | Connector names being acted on |
| `STILLWATCH_METHOD` | `dpms` | `dpms`, `overlay`, or `ddc_standby` |
| `STILLWATCH_REASON` | `blank` | `blank`, `resume`, `command`, or `panel_care` |

[lg-webos-off.sh](packaging/hooks/lg-webos-off.sh) and [lg-webos-on.sh](packaging/hooks/lg-webos-on.sh) call `lgtv screenOff` / `lgtv screenOn` ([LGWebOSRemote](https://github.com/klattimer/LGWebOSRemote)). Pair the TV once with `lgtv auth` before relying on them.

[cec-standby.sh](packaging/hooks/cec-standby.sh) and [cec-on.sh](packaging/hooks/cec-on.sh) send standard HDMI-CEC standby and image-view-on. They use `cec-ctl` when it's installed, otherwise `cec-client`. `CEC_DEVICE` picks the cec-ctl adapter (default `/dev/cec0`). One adapter, not one command per connector.

```toml
[action]
on_blank_cmd = "/usr/share/doc/stillwatch/hooks/lg-webos-off.sh"
on_resume_cmd = "/usr/share/doc/stillwatch/hooks/lg-webos-on.sh"
```

From a checkout, use the repo path instead of `/usr/share/doc/...`.

## Calibration

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

The GUI Calibration page is that grid as a heatmap. The page is still a placeholder, so probe is the calibration tool that runs today. `ignore_regions` in the settings schema is drawn on the same heatmap once the page exists.

## CLI

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

`stillwatch-gui` is the tray and the settings window. It talks to the same daemon. The window still opens when the daemon isn't running, and the tray icon changes until the daemon comes back (it reconnects on its own). A second `stillwatch-gui` hands off to the one already running instead of starting another tray. Quick snooze uses each `[prompt] snooze_presets_minutes` value. Settings is the schema-driven form. Calibration, History, Service, and `prompt` are still placeholders.

- **`status`**: state and time in it, snooze time left, idle/locked/media, the capture backend, the last stale check per output (persistent and dark percentages, threshold and why), panel care, and config errors if the last reload failed. `--json` prints one `StatusPayload` object.
- **`snooze <DURATION>`**: the daemon checks it against the `[prompt]` snooze rules (a preset, or `custom_min_minutes` to `custom_max_minutes` with `allow_custom`) and says why if it doesn't fit.
- **`cancel-snooze`**, **`pause`**, **`resume`**: what they say.
- **`reload`**: makes the daemon reload its config file and prints the result. If the new config is invalid, the problems are printed one per line, the daemon keeps the last good config, and the exit code is 1.
- **`history`**: see [Decision history](#decision-history).
- **`probe`**: see [Calibration](#calibration).
- **`idle-test`**: runs the daemon's idle and gamepad sources locally, no daemon needed, and prints a timestamped line each time the combined state changes: `idle`, `active (keyboard/mouse)`, or `active (gamepad: <name>)`. You only count as idle once the compositor reports input idle and no gamepad has moved past the deadzone for the timeout (default `idle.input_idle_minutes`). Ctrl-C stops it.
- **`config init`** / **`config check`**: write or validate the config file.

```
$ stillwatch idle-test --minutes 1
Watching keyboard, mouse, and gamepads with a 1m idle timeout. Ctrl-C to stop.
2026-10-02 20:41:07  idle
2026-10-02 20:41:30  active (gamepad: Xbox Wireless Controller)
2026-10-02 20:42:30  idle
2026-10-02 20:42:51  active (keyboard/mouse)
```

The daemon takes `--config <PATH>` (default `~/.config/stillwatch/config.toml`), `--log-level <LEVEL>`, `--capture-check <OUTPUT>`, and `--probe [--interval <DURATION>] [--count <N>]` (JSON lines for `stillwatch probe --standalone`). The log level comes from `--log-level`, then `RUST_LOG`, then `info`. Under systemd it logs to the journal, otherwise to stderr.

Colors are used only when stdout is a terminal and `NO_COLOR` isn't set.

Exit codes are the same for every command:

| Code | Meaning |
| -- | -- |
| 0 | Success |
| 1 | The command failed: the daemon refused it (the message says why), the config is invalid, or something else went wrong |
| 2 | Usage error (bad flags or arguments) |
| 3 | `stillwatchd` isn't running (`systemctl --user start stillwatch`) |

## Decision history

`stillwatch history` prints the ring in `~/.local/state/stillwatch/history.jsonl` (oldest first): prompts, blanks, snoozes, the safety ceiling, re-blanks, and reloads. Each row has the time, the event, the state change, per-output persistent percentages with the threshold and why, and the snooze, blank, or answer detail, plus media, gamepad, and lock context.

```
stillwatch history
stillwatch history --since 2h
stillwatch history --json
```

`--json` prints one `HistoryEntry` per line. The file holds numbers and state names only. No pixels, no window titles, no track metadata. `history.max_entries` (default 1000) drops the oldest first. The GUI History page reads the same file once that page exists. `stillwatch probe` is not history: probe is live, history is what already happened.

## Panel care

Stillwatch does not draw a pixel-exercise pattern. Lighting pixels only adds wear, and the "burn-in fix" patterns work by wearing down the healthy ones. There is no standard DDC or CEC pixel-cleaning command. Vendor codes that aren't in a published spec are unsafe to send (they can brick a panel), so none are built in.

What it does instead is let the panel's own compensation cycle run. Most OLEDs do that in real standby after they've been on for a while (Pixel Cleaning, on some monitors). Prefer `dpms` or `ddc_standby` over the overlay. The overlay keeps the signal up, so the panel stays on and the cycle doesn't start.

`[panel_care]` tracks screen-on time. `min_standby_minutes` in real standby resets the counter. The overlay doesn't. After `reminder_hours` (default 4) it reminds you to turn the display off. `trigger_cmd` runs at blank time when that threshold is due, for a model whose vendor published a tool. [panel-care-trigger.sh](packaging/hooks/panel-care-trigger.sh) is an empty skeleton with the warning in the header. It sends nothing until you replace the marked section.

Screen-on time is stored in `~/.local/state/stillwatch/panel.json`. `stillwatch status` prints the current counters.

## Privacy

Frames are downscaled to a luma grid and dropped. Pixels and luma values are never written to disk, logs, history, or D-Bus.

`stillwatch probe` and the calibration heatmap only carry per-block states (`changed`, `persistent`, `dark`, `ignored`) and percentages.

History and logs hold numbers and state names. No pixels, no window titles, no track metadata. Media in a history row is a playing flag, not a player name.

## Known limitations

- Mouse sensor jitter or desk vibration can keep the compositor from ever reporting input idle, so Stillwatch never starts the stale check.
- Reading, or a static dashboard, with no input, gets prompted. The countdown and snooze are the workaround. Per-app exemptions aren't in this version.
- A windowed video with no MPRIS player looks the same as a video call and can prompt. Players that don't export MPRIS, and browser players the integration extension doesn't see, hit this.

## Contributing

Toolchains come from [asdf](https://asdf-vm.com/) ([`.tool-versions`](.tool-versions)). The quality gates also need `cargo-nextest`, `cargo-llvm-cov`, `cargo-deny`, `cargo-machete`, and `jscpd` (`npm install -g jscpd`).

```sh
cargo xtask install-hooks   # pre-commit runs the fast gates (fmt, clippy, size)
cargo xtask ci               # every gate, in the same order as CI
cargo xtask ci --fast        # fmt, clippy, size only
cargo xtask ci --strict      # a missing tool fails the run
```

`install-hooks` points `core.hooksPath` at [`.githooks/`](.githooks). Those are git hooks, not the display hooks in `packaging/hooks/`.

One PR per Linear issue, against `main`. The branch name is the issue's `gitBranchName`. The PR body ends with `Closes JUS-N`. Squash merge, and only when CI is green. More of the layout is in [AGENTS.md](AGENTS.md).

Individual gates: `cargo xtask gate <fmt|clippy|size|jscpd|deny|machete|coverage|packaging|bench>`. A gate whose tool isn't installed is skipped with a message. `--strict` turns that into a failure. `packaging` runs `systemd-analyze verify --user` on `packaging/stillwatch.service` and `desktop-file-validate` on the desktop files. It also runs `shellcheck` on `packaging/hooks/*.sh` and `namcap` on the PKGBUILD when those two are installed. A missing `shellcheck` or `namcap` is a skip, including under `--strict`.

- `cargo xtask check-size [--max 400]` fails on any `.rs` file over 400 lines, not counting `#[cfg(test)]` items. Put big tests in a sibling `tests.rs` via `#[cfg(test)] mod tests;`.
- `cargo xtask coverage` runs the tests under `cargo llvm-cov nextest` and requires 80% line coverage for the workspace and 90% for `stillwatch-core`. Binary `main.rs` files and `xtask` are excluded. The lcov report lands in `target/coverage/lcov.info`.
- `jscpd` uses `.jscpd.json` (50 token minimum, tests excluded, any clone fails). `cargo deny` uses `deny.toml`.
- `cargo xtask gen-docs` regenerates `docs/config.md` from the settings schema. `--check` only verifies.

### Integration tests

Backend tests run against a real D-Bus and a real compositor, both started per test by `stillwatch-testkit`: a private `dbus-daemon`, and a headless `kwin_wayland --virtual` in a throwaway sandbox (own `XDG_RUNTIME_DIR`, home, and session bus). They never touch your desktop session, so they're safe to run inside Plasma. They need `dbus` and `kwin` installed and run with the rest under `cargo xtask coverage` / `cargo xtask ci`.

If either is missing the tests skip with a message. Set `STILLWATCH_REQUIRE_DBUS=1` and `STILLWATCH_REQUIRE_KWIN=1` to make that a failure instead (CI does).

```sh
cargo nextest run -p stillwatch-testkit -p stillwatchd   # just the crates with integration tests
STILLWATCH_REQUIRE_KWIN=1 cargo nextest run --test wayland_idle
```

### CI

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on pushes to `main` and on every PR, in an `archlinux:latest` container with the Rust version from `.tool-versions`. Both jobs call the same `cargo xtask` subcommands as above. The container installs `shellcheck` and `namcap`, so the packaging gate checks the hooks and the PKGBUILD there.

- **lint**: fmt, clippy, check-size, `docs/config.md` up to date, jscpd, cargo deny, cargo machete, the packaging checks (`systemd-analyze`, `desktop-file-validate`, `shellcheck`, `namcap`), and building the benches. Every gate runs even if an earlier one failed, so one push shows all of them.
- **test**: `cargo xtask coverage`, unit and integration tests together, with `STILLWATCH_REQUIRE_DBUS=1` and `STILLWATCH_REQUIRE_KWIN=1`. The lcov report is uploaded as the `lcov` artifact.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.
