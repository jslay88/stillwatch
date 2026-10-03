#!/bin/sh
# Example action.on_resume_cmd: turn an LG webOS TV's screen back on.
#
# Not wired up by install. Point on_resume_cmd at this file yourself.
# Stillwatch runs it as `sh -c` with a 10s timeout. A failure is logged
# and does not block wake.
#
# Needs `lgtv` from https://github.com/klattimer/LGWebOSRemote, already
# paired (`lgtv auth`). The command is `lgtv screenOn`. Another CLI
# means editing the exec line. Do not commit the TV's address.
#
# Environment, set by stillwatchd (empty if you run this by hand):
#   STILLWATCH_OUTPUTS  connector names, comma-separated (HDMI-A-1,DP-1)
#   STILLWATCH_METHOD   dpms, overlay, or ddc_standby
#   STILLWATCH_REASON   blank, resume, command, or panel_care

set -eu

: "${STILLWATCH_OUTPUTS:=}"
: "${STILLWATCH_METHOD:=}"
: "${STILLWATCH_REASON:=resume}"

if [ "$STILLWATCH_REASON" != "resume" ]; then
    printf 'lg-webos-on: STILLWATCH_REASON=%s, not turning the screen on\n' \
        "$STILLWATCH_REASON" >&2
    exit 0
fi

if ! command -v lgtv >/dev/null 2>&1; then
    printf 'lg-webos-on: lgtv is not on PATH\n' >&2
    exit 1
fi

printf 'lg-webos-on: outputs=%s method=%s\n' \
    "$STILLWATCH_OUTPUTS" "$STILLWATCH_METHOD" >&2
exec lgtv screenOn
