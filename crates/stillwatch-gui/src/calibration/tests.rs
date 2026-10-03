use std::fs;

use stillwatch_core::detector::{BlockRect, blocks_to_pixels, pixels_to_blocks};
use stillwatch_core::state::State;
use stillwatch_core::stats::{BlockState, Threshold, ThresholdReason};
use stillwatch_ipc::probe::{ProbeOutput, ProbeSample};

use super::{CalMsg, Calibration, ProbePace};
use crate::edit_msg::SettingsMsg;
use crate::model::update;
use crate::page::Page;
use crate::shell::{DaemonCall, DaemonEvent, Link, Message, Shell, Snapshot, Visibility};

fn shell_up() -> Shell {
    let mut shell = Shell::new(vec![15, 60]);
    shell.settings = Visibility::Open;
    shell.link = Link::Up(Snapshot {
        state: State::Monitoring,
        snooze_remaining_seconds: None,
        config_errors: Vec::new(),
    });
    let calls = update(
        &mut shell,
        Message::Daemon(DaemonEvent::Capture(Some("kwin".into()))),
    );
    assert_eq!(calls, Vec::new());
    shell
}

fn sample(
    width: u32,
    height: u32,
    columns: u16,
    rows: u16,
    blocks: Vec<BlockState>,
) -> ProbeSample {
    let mut output = ProbeOutput::from_blocks("DP-1", columns, rows, blocks, 70).unwrap();
    output.width = width;
    output.height = height;
    ProbeSample {
        at: jiff::Timestamp::UNIX_EPOCH,
        threshold: Threshold::new(70, ThresholdReason::Normal),
        stale: false,
        outputs: vec![output],
    }
}

#[test]
fn a_sample_becomes_heatmap_cells_and_a_summary() {
    let blocks = vec![
        BlockState::Persistent,
        BlockState::Changed,
        BlockState::Dark,
        BlockState::Ignored,
    ];
    let view = super::view_of(&sample(400, 400, 2, 2, blocks.clone()));
    assert_eq!(view.threshold_label(), "70% normal");
    assert!(!view.stale);
    let output = &view.outputs[0];
    assert_eq!(output.cells, blocks);
    assert_eq!(output.counted, 2);
    assert_eq!(output.persistent, 1);
    assert_eq!(output.dark, 1);
    assert_eq!(output.total, 4);
    assert_eq!(output.persistent_percent, "50%");
    assert_eq!(output.dark_percent, "25%");
    assert!(!output.stale);
    let summary = output.summary(&view.threshold_label());
    assert_eq!(
        summary,
        "DP-1: persistent 50% (dark 25%, counted 2/4), threshold 70% normal -> not stale"
    );
    assert!(!format!("{view:?}").contains("luma"));
    assert_ne!(
        super::draw::swatch(BlockState::Changed),
        super::draw::swatch(BlockState::Persistent)
    );
    assert_ne!(
        super::draw::swatch(BlockState::Dark),
        super::draw::swatch(BlockState::Ignored)
    );
}

#[test]
fn a_sample_whose_grid_does_not_match_is_dropped() {
    let mut output = ProbeOutput::from_blocks("DP-1", 2, 2, vec![BlockState::Dark; 4], 70).unwrap();
    output.blocks.pop();
    let view = super::view_of(&ProbeSample {
        at: jiff::Timestamp::UNIX_EPOCH,
        threshold: Threshold::new(90, ThresholdReason::Media),
        stale: false,
        outputs: vec![output],
    });
    assert_eq!(view.outputs.len(), 0);
    assert_eq!(view.threshold_label(), "90% media");
}

#[test]
fn cell_lookup_clamps_and_splits_uneven_widths() {
    assert_eq!(
        super::heat::cell_at(0.0, 0.0, 100.0, 10.0, 3, 1),
        Some((0, 0))
    );
    assert_eq!(
        super::heat::cell_at(33.4, 0.0, 100.0, 10.0, 3, 1),
        Some((1, 0))
    );
    assert_eq!(
        super::heat::cell_at(99.0, 9.0, 100.0, 10.0, 3, 1),
        Some((2, 0))
    );
    assert_eq!(
        super::heat::cell_at(-4.0, 0.0, 100.0, 10.0, 3, 1),
        Some((0, 0))
    );
    assert_eq!(
        super::heat::cell_at(500.0, 0.0, 100.0, 10.0, 3, 1),
        Some((2, 0))
    );
    assert_eq!(super::heat::cell_at(0.0, 0.0, 0.0, 10.0, 3, 1), None);
}

#[test]
fn rectangles_round_trip_when_the_size_does_not_divide_the_grid() {
    let blocks = BlockRect {
        column: 1,
        row: 0,
        columns: 1,
        rows: 1,
    };
    let pixels = blocks_to_pixels(blocks, 10, 10, 3, 1).unwrap();
    assert_eq!(pixels_to_blocks(pixels, 10, 10, 3, 1), Some(blocks));
    assert!(pixels.w > 0 && pixels.x > 0);

    let mut shell = shell_up();
    let _ = update(&mut shell, Message::Navigate(Page::Calibration));
    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::Probe(super::view_of(&sample(
            10,
            10,
            3,
            1,
            vec![
                BlockState::Changed,
                BlockState::Persistent,
                BlockState::Dark,
            ],
        )))),
    );
    let _ = update(
        &mut shell,
        Message::Calibration(CalMsg::BeginDrag {
            output: "DP-1".into(),
            column: 1,
            row: 0,
        }),
    );
    assert_eq!(
        update(&mut shell, Message::Calibration(CalMsg::FinishDrag)),
        Vec::new()
    );
    let region = &shell.editor.ignore_regions()[0];
    assert_eq!(region.output, "DP-1");
    assert_eq!(region.x, pixels.x.to_string());
    assert_eq!(region.y, pixels.y.to_string());
    assert_eq!(region.w, pixels.w.to_string());
    assert_eq!(region.h, pixels.h.to_string());
}

#[test]
fn regions_can_be_added_replaced_and_deleted() {
    let mut shell = shell_up();
    let _ = update(&mut shell, Message::Navigate(Page::Calibration));
    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::Probe(super::view_of(&sample(
            400,
            200,
            4,
            2,
            vec![BlockState::Changed; 8],
        )))),
    );
    drag(&mut shell, 0, 0, 1, 0);
    assert_eq!(shell.editor.ignore_regions().len(), 1);
    let first = shell.editor.ignore_regions()[0].clone();

    let _ = update(&mut shell, Message::Calibration(CalMsg::Edit(0)));
    drag(&mut shell, 2, 1, 3, 1);
    assert_eq!(shell.editor.ignore_regions().len(), 1);
    assert_ne!(shell.editor.ignore_regions()[0], first);
    assert!(shell.calibration.editing.is_none());

    assert_eq!(
        update(&mut shell, Message::Calibration(CalMsg::Delete(0))),
        Vec::new()
    );
    assert_eq!(shell.editor.ignore_regions().len(), 0);
}

#[test]
fn saving_a_drawn_region_writes_ignore_regions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut shell = shell_up();
    shell.config_path = Some(path.clone());
    let _ = update(&mut shell, Message::Navigate(Page::Calibration));
    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::Probe(super::view_of(&sample(
            400,
            400,
            4,
            4,
            vec![BlockState::Changed; 16],
        )))),
    );
    drag(&mut shell, 0, 0, 0, 0);
    let calls = update(&mut shell, Message::Settings(SettingsMsg::Save));
    assert_eq!(calls, vec![DaemonCall::Reload]);
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("output = \"DP-1\""), "{written}");
    assert!(written.contains("w = 100"), "{written}");
    assert!(written.contains("h = 100"), "{written}");
    let region_line = written
        .lines()
        .find(|line| line.contains("ignore_regions ="))
        .unwrap();
    assert!(!region_line.contains("luma"), "{region_line}");
}

#[test]
fn the_probe_starts_and_stops_with_the_page() {
    let mut shell = shell_up();
    assert_eq!(
        update(&mut shell, Message::Navigate(Page::Calibration)),
        vec![DaemonCall::StartProbe { interval_ms: 5_000 }]
    );
    assert!(shell.calibration.running);

    assert_eq!(
        update(
            &mut shell,
            Message::Calibration(CalMsg::Pace(ProbePace::OneSecond))
        ),
        vec![DaemonCall::StartProbe { interval_ms: 1_000 }]
    );
    assert_eq!(
        update(
            &mut shell,
            Message::Calibration(CalMsg::Pace(ProbePace::CheckInterval))
        ),
        vec![DaemonCall::StartProbe {
            interval_ms: 60_000
        }]
    );

    assert_eq!(
        update(&mut shell, Message::Navigate(Page::Settings)),
        vec![DaemonCall::StopProbe]
    );
    assert!(!shell.calibration.running);
    assert!(shell.calibration.view.is_none());
    assert_eq!(
        update(&mut shell, Message::Navigate(Page::History)),
        Vec::new()
    );
}

#[test]
fn closing_the_window_stops_the_probe_and_a_down_daemon_does_not() {
    let mut shell = shell_up();
    let _ = update(&mut shell, Message::Navigate(Page::Calibration));
    assert_eq!(
        update(&mut shell, Message::CloseSettings),
        vec![DaemonCall::StopProbe]
    );
    assert_eq!(shell.settings, Visibility::Closed);
    assert_eq!(update(&mut shell, Message::CloseSettings), Vec::new());

    let mut shell = shell_up();
    let _ = update(&mut shell, Message::Navigate(Page::Calibration));
    assert_eq!(
        update(&mut shell, Message::Daemon(DaemonEvent::Down)),
        Vec::new()
    );
    assert_eq!(shell.link, Link::Down);
    assert!(!shell.calibration.running);
    assert!(!shell.capture_known);

    let calls = update(
        &mut shell,
        Message::Daemon(DaemonEvent::Snapshot(Snapshot {
            state: State::Active,
            snooze_remaining_seconds: None,
            config_errors: Vec::new(),
        })),
    );
    assert!(calls.contains(&DaemonCall::RefreshDevices));
    assert!(calls.contains(&DaemonCall::StartProbe { interval_ms: 5_000 }));
}

#[test]
fn input_idle_only_mode_is_called_out() {
    assert_eq!(
        Calibration::idle_only_notice(true, true, None),
        Some(
            "No capture backend is available. Stillwatch is in input-idle-only mode, so there is no heatmap."
        )
    );
    assert_eq!(
        Calibration::idle_only_notice(true, true, Some("kwin")),
        None
    );
    assert_eq!(Calibration::idle_only_notice(true, false, None), None);
    assert_eq!(Calibration::idle_only_notice(false, true, None), None);

    let mut shell = shell_up();
    shell.capture_backend = None;
    shell.capture_known = true;
    shell.page = Page::Calibration;
    assert!(
        Calibration::idle_only_notice(
            matches!(shell.link, Link::Up(_)),
            shell.capture_known,
            shell.capture_backend.as_deref(),
        )
        .is_some()
    );
}

#[test]
fn a_drag_without_an_output_size_does_not_write_a_region() {
    let mut shell = shell_up();
    let _ = update(&mut shell, Message::Navigate(Page::Calibration));
    let _ = update(
        &mut shell,
        Message::Daemon(DaemonEvent::Probe(super::view_of(&sample(
            0,
            0,
            2,
            2,
            vec![BlockState::Changed; 4],
        )))),
    );
    drag(&mut shell, 0, 0, 1, 1);
    assert_eq!(shell.editor.ignore_regions().len(), 0);
    assert!(shell.calibration.place_error.is_some());
}

fn drag(shell: &mut Shell, column: u16, row: u16, end_column: u16, end_row: u16) {
    let _ = update(
        shell,
        Message::Calibration(CalMsg::BeginDrag {
            output: "DP-1".into(),
            column,
            row,
        }),
    );
    let _ = update(
        shell,
        Message::Calibration(CalMsg::MoveDrag {
            column: end_column,
            row: end_row,
        }),
    );
    let _ = update(shell, Message::Calibration(CalMsg::FinishDrag));
}
