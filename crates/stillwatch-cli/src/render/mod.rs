//! Human-readable output: the status summary, the history table, and the
//! probe grid.
//!
//! Every renderer is a pure function of a wire payload and a [`Style`], so
//! the same output can be produced from a D-Bus reply or anything else that
//! yields the payload types.

pub mod history;
pub mod probe;
pub mod status;

use std::env;
use std::ffi::OsStr;
use std::io::{self, IsTerminal as _};
use std::time::Duration;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use stillwatch_core::stats::{OutputStats, Threshold, ThresholdReason};

/// How output should look on the terminal it's going to.
#[derive(Debug, Clone)]
pub struct Style {
    /// Use ANSI colors.
    pub color: bool,
    /// Redraw live output in place instead of scrolling.
    pub redraw: bool,
    /// Time zone for timestamps.
    pub tz: TimeZone,
}

impl Style {
    /// No colors, no redraws.
    #[must_use]
    pub const fn plain(tz: TimeZone) -> Self {
        Self {
            color: false,
            redraw: false,
            tz,
        }
    }

    /// Colors and redraws only when stdout is a terminal (colors also need
    /// `NO_COLOR` unset or empty), with the system time zone.
    #[must_use]
    pub fn detect() -> Self {
        let tty = io::stdout().is_terminal();
        Self {
            color: wants_color(tty, env::var_os("NO_COLOR").as_deref()),
            redraw: tty,
            tz: TimeZone::system(),
        }
    }

    /// `at` as a local date and time.
    #[must_use]
    pub fn time(&self, at: Timestamp) -> String {
        at.to_zoned(self.tz.clone())
            .strftime("%Y-%m-%d %H:%M:%S")
            .to_string()
    }

    fn paint(&self, text: &str, paint: Paint) -> String {
        if self.color {
            format!("\x1b[{}m{text}\x1b[0m", paint.code())
        } else {
            text.to_owned()
        }
    }
}

/// <https://no-color.org>: any non-empty `NO_COLOR` turns colors off.
fn wants_color(tty: bool, no_color: Option<&OsStr>) -> bool {
    tty && no_color.is_none_or(OsStr::is_empty)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Paint {
    Red,
    Green,
    Blue,
    Dim,
    BoldRed,
}

impl Paint {
    const fn code(self) -> &'static str {
        match self {
            Self::Red => "31",
            Self::Green => "32",
            Self::Blue => "34",
            Self::Dim => "2",
            Self::BoldRed => "1;31",
        }
    }
}

/// `70% normal`
fn threshold(threshold: Threshold) -> String {
    let reason = match threshold.reason {
        ThresholdReason::Normal => "normal",
        ThresholdReason::Media => "media",
        ThresholdReason::Ceiling => "ceiling",
    };
    format!("{}% {reason}", threshold.percent)
}

/// A percentage rounded to a whole number, like `72%`.
fn percent(value: f64) -> String {
    format!("{value:.0}%")
}

/// `4m 12s`
fn duration(seconds: u64) -> String {
    humantime::format_duration(Duration::from_secs(seconds)).to_string()
}

fn verdict(is_stale: bool, style: &Style) -> String {
    if is_stale {
        style.paint("STALE", Paint::BoldRed)
    } else {
        style.paint("not stale", Paint::Green)
    }
}

/// One output's line, shared by the probe and status:
/// `HDMI-A-1: persistent 72% (dark 18%, counted 230/256), threshold 70% normal -> STALE`.
fn output_summary(stats: &OutputStats, counted: &str, applied: Threshold, style: &Style) -> String {
    format!(
        "{}: persistent {} (dark {}, counted {counted}), threshold {} -> {}",
        stats.output,
        percent(stats.persistent_percent),
        percent(stats.dark_percent),
        threshold(applied),
        verdict(stats.stale, style),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_needs_a_terminal_and_no_no_color() {
        assert!(wants_color(true, None));
        assert!(wants_color(true, Some(OsStr::new(""))));
        assert!(!wants_color(true, Some(OsStr::new("1"))));
        assert!(!wants_color(false, None));
    }

    #[test]
    fn detect_follows_stdout() {
        let style = Style::detect();
        assert_eq!(style.redraw, io::stdout().is_terminal());
        assert!(!style.color || style.redraw);
    }

    #[test]
    fn paint_only_with_color() {
        let mut style = Style::plain(TimeZone::UTC);
        assert_eq!(style.paint("x", Paint::Red), "x");
        style.color = true;
        assert_eq!(style.paint("x", Paint::Red), "\x1b[31mx\x1b[0m");
        assert_eq!(verdict(true, &style), "\x1b[1;31mSTALE\x1b[0m");
    }

    #[test]
    fn times_use_the_style_zone() {
        let at = Timestamp::from_second(1_790_000_000).unwrap();
        assert_eq!(Style::plain(TimeZone::UTC).time(at), "2026-09-21 14:13:20");
        let fixed = TimeZone::fixed(jiff::tz::offset(-6));
        assert_eq!(Style::plain(fixed).time(at), "2026-09-21 08:13:20");
    }

    #[test]
    fn numbers_format_compactly() {
        assert_eq!(percent(71.6), "72%");
        assert_eq!(percent(0.0), "0%");
        assert_eq!(duration(252), "4m 12s");
        assert_eq!(duration(0), "0s");
        assert_eq!(
            threshold(Threshold::new(98, ThresholdReason::Ceiling)),
            "98% ceiling"
        );
        assert_eq!(
            threshold(Threshold::new(90, ThresholdReason::Media)),
            "90% media"
        );
    }
}
