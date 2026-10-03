# Stillwatch

Context for anyone (human or agent) working in this repo. Enforceable rules live in `.cursor/rules/`; this file is background.

## What it is and why

Stillwatch protects OLED panels from burn-in on Linux Wayland. It watches for real input idle (keyboard, mouse, gamepad), confirms the screen is actually showing static content, prompts with a snooze option, and then blanks the displays (DPMS, DDC standby, or a black overlay).

- **Why not the desktop's own idle timer:** apps (video players, browsers, Discord calls) hold idle inhibitors, so the screen never turns off. Stillwatch uses `ext-idle-notify-v1` v2 *input* idle, which ignores inhibitors, and then decides from the pixels.
- **KDE-first Wayland:** KWin ScreenShot2 for capture, `kscreen-doctor` for DPMS. The xdg-desktop-portal ScreenCast path is the universal fallback. Other compositors come later.
- **Privacy:** frames are downscaled to a luma grid and dropped immediately. Pixels are never written to disk or sent anywhere (D-Bus, logs, history). Only per-block states (changed, persistent, dark, ignored) and percentages leave the detector.

## Crate map

| Path | What goes there |
| -- | -- |
| `crates/stillwatch-core` | Pure logic: config structs, validation, versioning, settings schema, stale detector, state machine, backend traits (contracts), `Clock`. No I/O apart from parsing. |
| `crates/stillwatch-ipc` | D-Bus interface `io.github.jslay88.Stillwatch` (zbus proxy + interface) and shared wire types (`Status`, `State`, `ProbeSample`). |
| `crates/stillwatchd` | The daemon. Backends (`idle/`, `gamepad/`, `media/`, `capture/`, `action/`, `overlay/`, `prompt/`, `session/`), the D-Bus service, platform detection. |
| `crates/stillwatch-cli` | The `stillwatch` binary: `status`, `snooze`, `pause`, `resume`, `reload`, `probe`, `idle-test`, `config init/check`, `history`. |
| `crates/stillwatch-gui` | The `stillwatch-gui` binary (iced): tray, schema-driven settings window, prompt dialog, calibration heatmap, History page. |
| `xtask` | Repo automation and quality gates (`cargo xtask ...`). |
| `packaging/` | systemd user unit, `.desktop` files, example display hooks, PKGBUILD. |

If logic could be shared by two crates, it belongs in `stillwatch-core` (pure) or `stillwatch-ipc` (wire types).

## Design: sans-IO core

The state machine and detector are pure. **Events go in, Commands come out.**

- Backends (idle source, gamepad, capture, prompter, blanker, session monitor, media watcher, history sink) implement traits defined in `stillwatch-core`. Each trait has a mock.
- The daemon owns the I/O: it turns Wayland/D-Bus/evdev activity into Events, feeds the core, and executes the returned Commands.
- Time comes from the injectable `Clock`, so every transition is tested with a fake clock and mocks, no compositor needed.

## Build and test

Toolchain comes from asdf via `.tool-versions` (Rust and Node, the latter for `jscpd`). Run `asdf install` in the repo root. CI reads the same file.

| Command | What it does |
| -- | -- |
| `cargo xtask ci` | Every gate exactly as CI runs it. Run before pushing. |
| `cargo xtask ci --fast` | The fast gates (fmt, clippy, size). Same as the pre-commit hook. |
| `cargo xtask install-hooks` | Points `core.hooksPath` at `.githooks/` so the pre-commit hook runs. |
| `cargo xtask check-size` | 400-line limit per `.rs` file, excluding `#[cfg(test)]` modules. |
| `cargo xtask coverage` | `cargo llvm-cov` with the thresholds (workspace >= 80%, `stillwatch-core` >= 90%). |
| `cargo xtask gen-docs` | Regenerates `docs/config.md` from the settings schema. Run it after changing any setting; `--check` only verifies. |

## Linear workflow

Team **JUS**, project **Stillwatch**. Every unit of work is a Linear issue sized for one PR.

1. Before starting: read the issue, its blockers, and the project documents it links.
2. Do the work on the issue's branch (see GitHub workflow).
3. When done: comment a summary on the issue (what changed, decisions, follow-ups).
4. File follow-ups as new issues rather than growing scope.
5. If a design changes, update the relevant project document and add an entry to the Decision log.

Project documents:

- [Architecture](https://linear.app/justin-slay/document/architecture-607efbe76361)
- [Detection algorithm](https://linear.app/justin-slay/document/detection-algorithm-15fc6616d35a)
- [State machine](https://linear.app/justin-slay/document/state-machine-37a6b0154462)
- [Settings and config](https://linear.app/justin-slay/document/settings-and-config-edc8709c7208)
- [Code quality standards](https://linear.app/justin-slay/document/code-quality-standards-60c98dc67af5)
- [Platform facts](https://linear.app/justin-slay/document/platform-facts-b327131e97cb)
- [Decision log](https://linear.app/justin-slay/document/decision-log-feaf408abedd)

## GitHub workflow

Repo: [jslay88/stillwatch](https://github.com/jslay88/stillwatch).

- Branch name is the issue's Linear `gitBranchName`, so GitHub and Linear link automatically.
- One PR per issue, opened against `main`.
- PR body ends with `Closes JUS-N`.
- Squash merge only when CI is green.

## Platform facts

What has been verified on the target machine (Plasma/KWin version, Wayland globals and versions, ScreenShot2 methods, DPMS, notification capabilities, the PG48UQ display and its DDC/CI) lives in the [Platform facts](https://linear.app/justin-slay/document/platform-facts-b327131e97cb) document, along with a "To verify" table. Check it before relying on platform behavior, and update it when you verify something new.
