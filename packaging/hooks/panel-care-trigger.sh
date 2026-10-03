#!/bin/sh
# Example panel_care.trigger_cmd.
#
# Not wired up by install. Point trigger_cmd at this file yourself, and
# only after you have replaced the marked section.
#
# WARNING: undocumented vendor codes can brick a panel. There is no
# standard DDC or CEC pixel-cleaning command. Stillwatch does not ship
# one, and this file does not send one. A forum VCP snippet is not a
# reason to put it here. Use a tool the manufacturer published for this
# exact model, or leave the skeleton as it is.
#
# Stillwatch runs this at blank time, and only when panel care is due
# (screen-on time past reminder_hours). The black overlay does not count
# as standby. The hook is `sh -c` with a 10s timeout. Failure is logged
# and does not block the blank.
#
# Environment, set by stillwatchd (empty if you run this by hand):
#   STILLWATCH_OUTPUTS  connector names, comma-separated (HDMI-A-1,DP-1)
#   STILLWATCH_METHOD   dpms, overlay, or ddc_standby
#   STILLWATCH_REASON   blank, resume, command, or panel_care

set -eu

: "${STILLWATCH_OUTPUTS:=}"
: "${STILLWATCH_METHOD:=}"
: "${STILLWATCH_REASON:=panel_care}"

if [ "$STILLWATCH_REASON" != "panel_care" ]; then
    printf 'panel-care-trigger: STILLWATCH_REASON=%s, not a panel care run\n' \
        "$STILLWATCH_REASON" >&2
    exit 0
fi

# --- model-specific command -----------------------------------------------
# Replace the printf with a published tool for this model. Do not put an
# undocumented VCP or CEC opcode here.
printf 'panel-care-trigger: no model-specific command configured; sending nothing\n' >&2
printf 'panel-care-trigger: outputs=%s method=%s\n' \
    "$STILLWATCH_OUTPUTS" "$STILLWATCH_METHOD" >&2
exit 0
