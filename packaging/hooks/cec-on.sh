#!/bin/sh
# Example action.on_resume_cmd: HDMI-CEC image view on.
#
# Not wired up by install. Point on_resume_cmd at this file yourself.
# Stillwatch runs it as `sh -c` with a 10s timeout. A failure is logged
# and does not block wake.
#
# Uses `cec-ctl` (v4l-utils) when it is on PATH, otherwise `cec-client`
# (libcec). cec-ctl sends Image View On to logical address 0 (the TV).
# cec-client sends `on 0`. Both are standard CEC, not a vendor opcode.
# If the TV wakes on the wrong input, add `--active-source` to the
# cec-ctl line for that set.
#
# One adapter, not one command per connector. STILLWATCH_OUTPUTS lists
# the connectors Stillwatch woke. CEC_DEVICE picks the adapter (default
# /dev/cec0) for cec-ctl. cec-client uses its default adapter.
#
# Environment, set by stillwatchd (empty if you run this by hand):
#   STILLWATCH_OUTPUTS  connector names, comma-separated (HDMI-A-1,DP-1)
#   STILLWATCH_METHOD   dpms, overlay, or ddc_standby
#   STILLWATCH_REASON   blank, resume, command, or panel_care

set -eu

: "${STILLWATCH_OUTPUTS:=}"
: "${STILLWATCH_METHOD:=}"
: "${STILLWATCH_REASON:=resume}"
: "${CEC_DEVICE:=/dev/cec0}"

if [ "$STILLWATCH_REASON" != "resume" ]; then
    printf 'cec-on: STILLWATCH_REASON=%s, not sending image view on\n' \
        "$STILLWATCH_REASON" >&2
    exit 0
fi

printf 'cec-on: outputs=%s method=%s device=%s\n' \
    "$STILLWATCH_OUTPUTS" "$STILLWATCH_METHOD" "$CEC_DEVICE" >&2

if command -v cec-ctl >/dev/null 2>&1; then
    exec cec-ctl --device="$CEC_DEVICE" --playback --to 0 --image-view-on -s
fi

if command -v cec-client >/dev/null 2>&1; then
    printf 'on 0\n' | cec-client -s -d 1
    exit
fi

printf 'cec-on: install cec-ctl (v4l-utils) or cec-client (libcec)\n' >&2
exit 1
