//! `[stale]`

use super::{Choice, Control, Section, Setting, TimeUnit};
use crate::config::limits::{DOWNSCALE_WIDTH, LUMA, PERCENT, PERCENT_NONZERO, POSITIVE};

pub(super) const SECTION: Section = Section {
    id: "stale",
    title: "Stale detection",
    help: "The per-block persistence detector that decides whether the screen is static. \
           Each output is split into blocks; a block is persistent once it has stayed \
           unchanged for `persist_checks` captures in a row. Use `stillwatch probe` or the \
           calibration heatmap to tune these.",
    settings: &[
        Setting::new(
            "stale.check_interval_seconds",
            "Check interval",
            Control::Duration {
                unit: TimeUnit::Seconds,
                bounds: POSITIVE,
            },
            "Seconds between captures while you're idle.",
        ),
        Setting::new(
            "stale.persist_checks",
            "Captures to persist",
            Control::Int {
                bounds: POSITIVE,
                step: 1,
            },
            "Captures in a row a block must stay unchanged to count as persistent. This is \
             what rejects motion, so video and animations don't trigger. The normal path to \
             a prompt takes `persist_checks * check_interval_seconds`.",
        ),
        Setting::new(
            "stale.stale_percent",
            "Stale threshold",
            Control::Percent {
                bounds: PERCENT_NONZERO,
            },
            "Percentage of counted (non-dark, non-ignored) blocks that must be persistent \
             for an output to be stale. The default of 70 is lower than an area-only check \
             would need, because per-block persistence already rejects motion. That way a \
             call with video tiles over a static screen still triggers.",
        ),
        Setting::new(
            "stale.media_stale_percent",
            "Threshold while media plays",
            Control::Percent { bounds: PERCENT },
            "Used instead of `stale_percent` while a non-ignored MPRIS player is Playing. \
             Pixels alone can't tell a windowed video you're watching from a call left \
             running, so playback raises the bar. 0 disables it.",
        ),
        Setting::new(
            "stale.media_ignore_players",
            "Ignored media players",
            Control::PlayerPicker,
            "MPRIS players that don't raise the threshold, such as audio-only players.",
        ),
        Setting::new(
            "stale.luma_delta_threshold",
            "Change tolerance",
            Control::Int {
                bounds: LUMA,
                step: 1,
            },
            "Largest change in a block's mean luma that still counts as unchanged. Absorbs \
             capture noise and dithering.",
        ),
        Setting::new(
            "stale.ignore_dark_below",
            "Dark block cutoff",
            Control::Int {
                bounds: LUMA,
                step: 1,
            },
            "Blocks darker than this luma in both the previous and the current capture are \
             left out of the count entirely, because black pixels are off on OLED and don't \
             wear. An all-dark screen is never stale. 0 disables it.",
        ),
        Setting::new(
            "stale.downscale_width",
            "Luma grid width",
            Control::Int {
                bounds: DOWNSCALE_WIDTH,
                step: 16,
            },
            "Width in pixels each frame is downscaled to before blocks are measured. The \
             height follows the output's aspect ratio.",
        ),
        Setting::new(
            "stale.block_grid",
            "Block grid",
            Control::GridSize { bounds: POSITIVE },
            "Blocks per output as `[cols, rows]`, each at most `downscale_width`. A toast or \
             a new chat message only resets the blocks it touches.",
        )
        .resetting_detection(),
        Setting::new(
            "stale.monitored_outputs",
            "Monitored outputs",
            Control::OutputPicker,
            "Outputs to watch, by connector name such as `HDMI-A-1`. Empty watches all of \
             them. Use it to skip LCDs in a mixed OLED and LCD setup.",
        )
        .resetting_detection(),
        Setting::new(
            "stale.require",
            "Outputs that must be stale",
            Control::Enum {
                choices: &[
                    Choice {
                        value: "all",
                        label: "All",
                        help: "Every monitored output must be stale.",
                    },
                    Choice {
                        value: "any",
                        label: "Any",
                        help: "One stale output is enough.",
                    },
                ],
            },
            "How many monitored outputs must be stale before prompting.",
        ),
        Setting::new(
            "stale.ignore_regions",
            "Ignored regions",
            Control::RegionEditor,
            "Rectangles the detector skips, such as a panel clock or a tray icon that always \
             changes. Each is `{ output, x, y, w, h }` in output pixels, with `w` and `h` at \
             least 1.",
        ),
    ],
};
