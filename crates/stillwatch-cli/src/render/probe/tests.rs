use jiff::Timestamp;
use jiff::tz::TimeZone;
use stillwatch_core::stats::{Threshold, ThresholdReason};

use super::*;

use BlockState::{Changed as C, Dark as D, Ignored as I, Persistent as P};

fn sample(outputs: Vec<ProbeOutput>) -> ProbeSample {
    ProbeSample {
        at: Timestamp::from_second(1_790_000_000).unwrap(),
        threshold: Threshold::new(70, ThresholdReason::Normal),
        stale: outputs.iter().all(|output| output.stats.stale),
        outputs,
    }
}

fn hdmi() -> ProbeOutput {
    let blocks = vec![P, P, P, C, P, P, D, D, I, P, P, D];
    ProbeOutput::from_blocks("HDMI-A-1", 4, 3, blocks, 70).unwrap()
}

fn plain() -> Style {
    Style::plain(TimeZone::UTC)
}

#[test]
fn grid_snapshot() {
    assert_eq!(
        render(&sample(vec![hdmi()]), &plain()),
        "\
2026-09-21 14:13:20  screen: STALE

HDMI-A-1: persistent 88% (dark 25%, counted 8/12), threshold 70% normal -> STALE
██████░░
████····
xx████··

██ persistent  ░░ changed  ·· dark  xx ignored
"
    );
}

#[test]
fn every_output_gets_a_summary_and_grid() {
    let dp = ProbeOutput::from_blocks("DP-1", 2, 1, vec![C, P], 70).unwrap();
    let mut sample = sample(vec![hdmi(), dp]);
    sample.threshold = Threshold::new(90, ThresholdReason::Media);
    let text = render(&sample, &plain());
    assert!(text.starts_with("2026-09-21 14:13:20  screen: not stale\n"));
    assert!(text.contains(
        "\nDP-1: persistent 50% (dark 0%, counted 2/2), threshold 90% media -> not stale\n░░██\n"
    ));
    assert!(text.contains("threshold 90% media -> STALE\n██████░░\n"));
}

#[test]
fn no_outputs_says_so() {
    let text = render(&sample(Vec::new()), &plain());
    assert_eq!(
        text,
        "2026-09-21 14:13:20  screen: STALE\n\nno monitored outputs\n\n\
         ██ persistent  ░░ changed  ·· dark  xx ignored\n"
    );
}

#[test]
fn colors_paint_runs_not_blocks() {
    let style = Style {
        color: true,
        ..plain()
    };
    let output = ProbeOutput::from_blocks("DP-1", 3, 1, vec![P, P, C], 70).unwrap();
    assert_eq!(
        grid(&output, &style),
        "\x1b[31m████\x1b[0m\x1b[32m░░\x1b[0m\n"
    );
    let legend = legend(&style);
    assert!(legend.starts_with("\x1b[31m██\x1b[0m persistent  \x1b[32m░░\x1b[0m changed"));
    assert!(legend.contains("\x1b[2m··\x1b[0m dark  \x1b[34mxx\x1b[0m ignored"));
}

#[test]
fn a_short_block_list_still_renders() {
    let mut output = hdmi();
    output.blocks.truncate(5);
    assert_eq!(grid(&output, &plain()), "██████░░\n██\n");
    output.columns = 0;
    output.blocks = vec![D];
    assert_eq!(grid(&output, &plain()), "··\n");
}
