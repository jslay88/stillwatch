# Configuration reference

<!-- Generated from the settings schema in crates/stillwatch-core/src/schema.
     Run `cargo xtask gen-docs` after changing it; a test fails when this file is stale. -->

Stillwatch reads `$XDG_CONFIG_HOME/stillwatch/config.toml`, normally
`~/.config/stillwatch/config.toml`. Every key is optional: missing keys and
sections take the defaults below, and unknown keys are rejected. The daemon
runs on defaults when the file doesn't exist.

- `stillwatch config init [--force] [PATH]` writes a commented copy of the defaults.
- `stillwatch config check [PATH]` validates a file without the daemon.
- Keys marked **resets detection** rebuild the capture backend and reset the
  block counters when changed. Everything else applies on reload.

## Sections

- [General](#general): top-level keys
- [Idle](#idle): `[idle]`
- [Locked session](#locked-session): `[session]`
- [Gamepads](#gamepads): `[activity]`
- [Capture](#capture): `[capture]`
- [Stale detection](#stale-detection): `[stale]`
- [Snooze ceiling](#snooze-ceiling): `[safety]`
- [Prompt](#prompt): `[prompt]`
- [Action](#action): `[action]`
- [Panel care](#panel-care): `[panel_care]`
- [History](#history): `[history]`
- [Logging](#logging): `[logging]`

## General

Keys outside any section.

### `version`

**Config version.** Config schema version. Older files are migrated in memory, and files newer than this build understands are refused. Leave it alone: the settings window writes the new version when it saves a migrated file.

- Type: read-only integer
- Default: `1`

## Idle

When you count as away.

### `idle.input_idle_minutes`

**Input idle time.** Minutes without keyboard, mouse, or gamepad input before Stillwatch starts checking the screen. This is real input idle from the compositor, so apps holding idle inhibitors (video players, browsers, calls) don't stop it from counting.

- Type: duration in minutes
- Default: `10`
- Allowed: at least 1

## Locked session

What happens while the session is locked.

### `session.when_locked`

**When locked.** What to do while the session is locked.

- Type: choice
- Default: `"blank_after"`
- Values:
  - `pause`: Do nothing until you unlock. Blanking is left to the desktop's own lock screen settings.
  - `blank_after`: Blank once the session has been locked for `locked_blank_seconds`, with no stale check or prompt. The lock screen is static by design.

### `session.locked_blank_seconds`

**Blank after locked for.** Seconds the session has to stay locked before blanking, with `when_locked = "blank_after"`. Input while locked wakes the display, and it blanks again after the same delay if the session stays locked.

- Type: duration in seconds
- Default: `60`

## Gamepads

Gamepad input as activity. Compositors don't count gamepads as input, so Stillwatch reads joystick devices itself.

### `activity.gamepad`

**Gamepad counts as input.** Count gamepad buttons and sticks as activity, so playing with a controller doesn't look like being away.

- Type: boolean
- Default: `true`

### `activity.gamepad_deadzone_percent`

**Stick deadzone.** Stick movement below this percentage of full travel is ignored, so a drifting stick doesn't keep you active forever.

- Type: percent
- Default: `15`
- Allowed: 0 to 100

### `activity.gamepad_ignore_devices`

**Ignored gamepads.** Gamepads to ignore, matched by name substring. Use it for a pad with bad drift or a sim rig that reports all the time.

- Type: list of gamepad name substrings
- Default: `[]`

### `activity.gamepad_wakes_display`

**Gamepad wakes displays.** Wake blanked displays on gamepad input. The compositor only wakes them for keyboard and mouse, so Stillwatch does it itself.

- Type: boolean
- Default: `true`

## Capture

How the screen is read. Frames are downscaled to a luma grid and dropped right away; pixels are never stored or sent anywhere.

### `capture.backend`

**Capture backend.** Which capture backend reads the screen.

- Type: choice
- Default: `"auto"`
- Values:
  - `auto`: KWin ScreenShot2 when it's available, otherwise the portal.
  - `kwin`: KDE's screenshot interface. No screen sharing indicator.
  - `portal`: xdg-desktop-portal ScreenCast. Works on any desktop. The stream only runs while you're away.
- Resets detection

## Stale detection

The per-block persistence detector that decides whether the screen is static. Each output is split into blocks; a block is persistent once it has stayed unchanged for `persist_checks` captures in a row. Use `stillwatch probe` or the calibration heatmap to tune these.

### `stale.check_interval_seconds`

**Check interval.** Seconds between captures while you're idle.

- Type: duration in seconds
- Default: `60`
- Allowed: at least 1

### `stale.persist_checks`

**Captures to persist.** Captures in a row a block must stay unchanged to count as persistent. This is what rejects motion, so video and animations don't trigger. The normal path to a prompt takes `persist_checks * check_interval_seconds`.

- Type: integer
- Default: `5`
- Allowed: at least 1

### `stale.stale_percent`

**Stale threshold.** Percentage of counted (non-dark, non-ignored) blocks that must be persistent for an output to be stale. The default of 70 is lower than an area-only check would need, because per-block persistence already rejects motion. That way a call with video tiles over a static screen still triggers.

- Type: percent
- Default: `70`
- Allowed: 1 to 100

### `stale.media_stale_percent`

**Threshold while media plays.** Used instead of `stale_percent` while a non-ignored MPRIS player is Playing. Pixels alone can't tell a windowed video you're watching from a call left running, so playback raises the bar. 0 disables it.

- Type: percent
- Default: `90`
- Allowed: 0 to 100

### `stale.media_ignore_players`

**Ignored media players.** MPRIS players that don't raise the threshold, such as audio-only players. An entry matches a bus-name suffix (`firefox` covers `firefox.instance_1_42`) or the player's Identity, such as `VLC media player`.

- Type: list of MPRIS player names
- Default: `["spotify"]`

### `stale.luma_delta_threshold`

**Change tolerance.** Largest change in a block's mean luma that still counts as unchanged. Absorbs capture noise and dithering.

- Type: integer
- Default: `6`
- Allowed: 0 to 255

### `stale.ignore_dark_below`

**Dark block cutoff.** Blocks darker than this luma in both the previous and the current capture are left out of the count entirely, because black pixels are off on OLED and don't wear. An all-dark screen is never stale. 0 disables it.

- Type: integer
- Default: `16`
- Allowed: 0 to 255

### `stale.downscale_width`

**Luma grid width.** Width in pixels each frame is downscaled to before blocks are measured. The height follows the output's aspect ratio.

- Type: integer
- Default: `480`
- Allowed: at least 16

### `stale.block_grid`

**Block grid.** Blocks per output as `[cols, rows]`, each at most `downscale_width`. A toast or a new chat message only resets the blocks it touches.

- Type: grid size (cols, rows)
- Default: `[16, 16]`
- Allowed: each at least 1
- Resets detection

### `stale.monitored_outputs`

**Monitored outputs.** Outputs to watch, by connector name such as `HDMI-A-1`. Empty watches all of them. Use it to skip LCDs in a mixed OLED and LCD setup.

- Type: list of output names
- Default: `[]`
- Resets detection

### `stale.require`

**Outputs that must be stale.** How many monitored outputs must be stale before prompting.

- Type: choice
- Default: `"all"`
- Values:
  - `all`: Every monitored output must be stale.
  - `any`: One stale output is enough.

### `stale.ignore_regions`

**Ignored regions.** Rectangles the detector skips, such as a panel clock or a tray icon that always changes. Each is `{ output, x, y, w, h }` in output pixels, with `w` and `h` at least 1.

- Type: list of regions
- Default: `[]`

## Snooze ceiling

A backstop for a snooze left running over a static screen. Captures keep running while you're idle during a snooze, and the prompt comes back if the screen has been static for too long.

### `safety.ceiling_enabled`

**Snooze ceiling.** Let the ceiling end a snooze early.

- Type: boolean
- Default: `true`

### `safety.ceiling_minutes`

**Ceiling time.** Minutes blocks must stay unchanged during a snooze before the prompt comes back. Must be longer than the normal path (`persist_checks * check_interval_seconds`).

- Type: duration in minutes
- Default: `30`

### `safety.ceiling_stale_percent`

**Ceiling threshold.** Percentage of counted blocks that must have been unchanged for `ceiling_minutes`. Higher than `stale_percent`, because it overrides a snooze you asked for.

- Type: percent
- Default: `98`
- Allowed: 1 to 100

### `safety.ceiling_during_pause`

**Ceiling while paused.** Apply the ceiling while paused too, not just while snoozed.

- Type: boolean
- Default: `false`

## Prompt

The prompt shown before acting, with its countdown and snooze choices.

### `prompt.style`

**Prompt style.** How the prompt is shown.

- Type: choice
- Default: `"auto"`
- Values:
  - `auto`: A notification normally. The dialog during fullscreen only after notifications are verified not to show there.
  - `notification`: A desktop notification with snooze actions and a live countdown.
  - `dialog`: Stillwatch's own dialog window.

### `prompt.urgency`

**Notification urgency.** Urgency of the prompt notification. Critical keeps it visible in Do Not Disturb.

- Type: choice
- Default: `"critical"`
- Values:
  - `low`: Low urgency.
  - `normal`: Normal urgency.
  - `critical`: Critical urgency, which Plasma shows even in Do Not Disturb.

### `prompt.fallback_to_dialog`

**Fall back to the dialog.** Show the dialog when there's no notification server, the notification fails, or it's closed without picking an action.

- Type: boolean
- Default: `true`

### `prompt.countdown_seconds`

**Countdown.** Seconds the prompt waits for an answer before the action runs. After any input the action won't run: the prompt waits up to `answer_grace_seconds` for an answer, never past the countdown, then closes.

- Type: duration in seconds
- Default: `60`
- Allowed: at least 1

### `prompt.answer_grace_seconds`

**Time to answer after input.** Seconds the prompt stays up after the first input, so the mouse move or key press that reaches it doesn't close it before you click. Picking Custom... restarts it. Unanswered, the prompt closes and nothing happens.

- Type: duration in seconds
- Default: `10`
- Allowed: 1 to 120

### `prompt.snooze_presets_minutes`

**Snooze buttons.** Snooze choices on the prompt, in minutes. Each must be between `custom_min_minutes` and `custom_max_minutes`. Can only be empty when `allow_custom` is on.

- Type: list of durations in minutes
- Default: `[15, 60, 180]`
- Allowed: each at least 1

### `prompt.allow_custom`

**Custom snooze.** Offer a Custom... choice that opens the dialog to pick any duration.

- Type: boolean
- Default: `true`

### `prompt.custom_min_minutes`

**Shortest snooze.** Shortest snooze allowed, in minutes.

- Type: duration in minutes
- Default: `1`
- Allowed: at least 1

### `prompt.custom_max_minutes`

**Longest snooze.** Longest snooze allowed, in minutes. Must be at least `custom_min_minutes`.

- Type: duration in minutes
- Default: `720`

### `prompt.snooze_cancelled_by_input`

**Input ends a snooze.** End a snooze as soon as there's input. Off keeps the snooze for the whole time you picked.

- Type: boolean
- Default: `false`

## Action

What happens when the prompt times out, plus hooks and the re-blank watchdog.

### `action.mode`

**Action.** What happens when the prompt times out.

- Type: choice
- Default: `"blank"`
- Values:
  - `blank`: Blank the displays.
  - `lock_and_blank`: Lock the session, then blank.
  - `dim_then_blank`: Dim for `dim_seconds`, then blank. Input while dimmed cancels.
  - `command`: Run `command` instead of blanking.

### `action.blank_method`

**Blank method.** How displays are blanked. Prefer a method that reaches real standby, so the panel's own care cycle can run. On KWin, partial DPMS becomes an overlay blank of the targets.

- Type: choice
- Default: `"dpms"`
- Values:
  - `dpms`: Turn the signal off through the compositor. The panel reaches standby, so its own compensation cycle (Pixel Cleaning on some monitors) can run. KWin applies DPMS to every output. A partial target list is blanked with the overlay instead.
  - `overlay`: Cover each output with a black surface. Works everywhere and keeps the signal alive for TVs, but keeps the panel on and blocks panel compensation.
  - `ddc_standby`: Send the standard MCCS power mode command (VCP 0xD6) over DDC/CI. Real standby on monitors that support it.

### `action.outputs`

**Outputs to act on.** Which outputs the action applies to.

- Type: choice
- Default: `"monitored"`
- Values:
  - `monitored`: Only `stale.monitored_outputs` (every output when that's empty).
  - `all`: Every connected output.

### `action.dim_method`

**Dim method.** How the dim step of `dim_then_blank` dims.

- Type: choice
- Default: `"overlay"`
- Values:
  - `overlay`: A translucent black overlay.
  - `brightness`: Lower the screen brightness (KDE).

### `action.dim_percent`

**Dim level.** How bright the screen stays while dimmed, as a percentage.

- Type: percent
- Default: `20`
- Allowed: 0 to 100

### `action.dim_seconds`

**Dim time.** Seconds to stay dimmed before blanking.

- Type: duration in seconds
- Default: `30`

### `action.command`

**Action command.** Command run instead of blanking with `mode = "command"`. Required in that mode.

- Type: command
- Default: `""`

### `action.on_blank_cmd`

**After blanking.** Command run after the displays are blanked, such as a TV screen-off utility. Runs as `sh -c` with a 10s timeout. `STILLWATCH_OUTPUTS`, `STILLWATCH_METHOD`, and `STILLWATCH_REASON` are set. Failures are logged and never block.

- Type: command
- Default: `""`

### `action.on_resume_cmd`

**On resume.** Command run when the displays wake. Same `sh -c` timeout and environment as `on_blank_cmd` (`STILLWATCH_REASON=resume`).

- Type: command
- Default: `""`

### `action.reblank_on_wake`

**Re-blank on wake.** Blank again when a display wakes without any input, such as a monitor that wakes itself when the HDMI link drops.

- Type: boolean
- Default: `true`

### `action.reblank_grace_seconds`

**Re-blank delay.** Seconds to wait after an unexpected wake before blanking again.

- Type: duration in seconds
- Default: `15`

### `action.reblank_max_attempts`

**Re-blank attempts.** Re-blank attempts before switching to `reblank_fallback`. 0 means unlimited.

- Type: integer
- Default: `3`

### `action.reblank_fallback`

**After the last attempt.** What to do when re-blanking keeps failing.

- Type: choice
- Default: `"overlay"`
- Values:
  - `overlay`: Switch to the black overlay, which keeps the signal alive so the display can't wake itself.
  - `none`: Stop re-blanking.

## Panel care

Helps the display's own compensation cycle run, which most OLED panels do in standby after cumulative use. Stillwatch never draws pixel-exercise patterns: lighting pixels only adds wear.

### `panel_care.enabled`

**Track screen-on time.** Track how long the displays have been on since their last real standby.

- Type: boolean
- Default: `true`

### `panel_care.min_standby_minutes`

**Standby that resets.** Minutes in standby that reset screen-on time. The black overlay doesn't count, since the panel stays on.

- Type: duration in minutes
- Default: `10`

### `panel_care.reminder_enabled`

**Reminder.** Once screen-on time passes `reminder_hours`, remind you that turning the display off lets panel care run.

- Type: boolean
- Default: `true`

### `panel_care.reminder_hours`

**Remind after.** Screen-on hours before the reminder.

- Type: duration in hours
- Default: `4`
- Allowed: at least 1

### `panel_care.trigger_cmd`

**Panel care command.** Command run at blank time when panel care is due, for displays with a model-specific command. No vendor codes are built in, because sending undocumented codes is unsafe.

- Type: command
- Default: `""`

## History

The decision history behind `stillwatch history` and the History page. It holds numbers, state names, and output connector names only: no pixels, window titles, or track metadata.

### `history.enabled`

**Record history.** Record every prompt, blank, snooze, ceiling trigger, re-blank, and reload, with the numbers behind it.

- Type: boolean
- Default: `true`

### `history.max_entries`

**Entries kept.** Entries kept in `~/.local/state/stillwatch/history.jsonl`. The oldest are dropped first.

- Type: integer
- Default: `1000`
- Allowed: at least 1

## Logging

Daemon log output.

### `logging.level`

**Log level.** How much the daemon logs.

- Type: choice
- Default: `"info"`
- Values:
  - `error`: Errors only.
  - `warn`: Warnings and errors.
  - `info`: Informational messages and above.
  - `debug`: Debug messages and above.
  - `trace`: Everything.
