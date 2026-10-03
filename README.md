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
stillwatch idle-test [--timeout <DURATION>]
stillwatch config init [--force] [--path <PATH>]
stillwatch config check [PATH]
```

Most commands are stubs until the daemon's D-Bus service lands. `stillwatch <command> --help` has details.

The daemon takes `--config <PATH>` (default `~/.config/stillwatch/config.toml`) and `--log-level <LEVEL>`. The log level comes from `--log-level`, then `RUST_LOG`, then `info` (the config's `logging.level` will slot in before the default once the daemon loads its config). Under systemd it logs to the journal, otherwise to stderr.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.
