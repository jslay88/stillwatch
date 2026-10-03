#!/bin/sh
# Example action.on_blank_cmd: turn an LG webOS TV's screen off.
#
# Not wired up by install. Point on_blank_cmd at this file yourself.
# Stillwatch runs it as `sh -c` with a 10s timeout. A failure is logged
# and does not block blanking.
#
# Needs `lgtv` from https://github.com/klattimer/LGWebOSRemote, already
# paired (`lgtv auth`). The command is `lgtv screenOff`. Another CLI
# (bscpylgtvcommand's `turn_screen_off`, for example) means editing the
# exec line. Do not commit the TV's address.
#
# Environment, set by stillwatchd (empty if you run this by hand):
#   STILLWATCH_OUTPUTS  connector names, comma-separated (HDMI-A-1,DP-1)
#   STILLWATCH_METHOD   dpms, overlay, or ddc_standby
#   STILLWATCH_REASON   blank, resume, command, or panel_care

set -eu

: "${STILLWATCH_OUTPUTS:=}"
: "${STILLWATCH_METHOD:=}"
: "${STILLWATCH_REASON:=blank}"

if [ "$STILLWATCH_REASON" != "blank" ]; then
    printf 'lg-webos-off: STILLWATCH_REASON=%s, not turning the screen off\n' \
        "$STILLWATCH_REASON" >&2
    exit 0
fi

if ! command -v lgtv >/dev/null 2>&1; then
    printf 'lg-webos-off: lgtv is not on PATH\n' >&2
    exit 1
fi

printf 'lg-webos-off: outputs=%s method=%s\n' \
    "$STILLWATCH_OUTPUTS" "$STILLWATCH_METHOD" >&2
exec lgtv screenOff
