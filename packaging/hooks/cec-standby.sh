#!/bin/sh
# Example action.on_blank_cmd: HDMI-CEC standby.
#
# Not wired up by install. Point on_blank_cmd at this file yourself.
# Stillwatch runs it as `sh -c` with a 10s timeout. A failure is logged
# and does not block blanking.
#
# Uses `cec-ctl` (v4l-utils) when it is on PATH, otherwise `cec-client`
# (libcec). Both send the standard CEC Standby message to logical
# address 0 (the TV). This is not a vendor opcode.
#
# One adapter, not one command per connector. STILLWATCH_OUTPUTS lists
# the connectors Stillwatch blanked. CEC_DEVICE picks the adapter
# (default /dev/cec0) for cec-ctl. cec-client uses its default adapter.
#
# Environment, set by stillwatchd (empty if you run this by hand):
#   STILLWATCH_OUTPUTS  connector names, comma-separated (HDMI-A-1,DP-1)
#   STILLWATCH_METHOD   dpms, overlay, or ddc_standby
#   STILLWATCH_REASON   blank, resume, command, or panel_care

set -eu

: "${STILLWATCH_OUTPUTS:=}"
: "${STILLWATCH_METHOD:=}"
: "${STILLWATCH_REASON:=blank}"
: "${CEC_DEVICE:=/dev/cec0}"

if [ "$STILLWATCH_REASON" != "blank" ]; then
    printf 'cec-standby: STILLWATCH_REASON=%s, not sending standby\n' \
        "$STILLWATCH_REASON" >&2
    exit 0
fi

printf 'cec-standby: outputs=%s method=%s device=%s\n' \
    "$STILLWATCH_OUTPUTS" "$STILLWATCH_METHOD" "$CEC_DEVICE" >&2

if command -v cec-ctl >/dev/null 2>&1; then
    exec cec-ctl --device="$CEC_DEVICE" --playback --to 0 --standby -s
fi

if command -v cec-client >/dev/null 2>&1; then
    printf 'standby 0\n' | cec-client -s -d 1
    exit
fi

printf 'cec-standby: install cec-ctl (v4l-utils) or cec-client (libcec)\n' >&2
exit 1
