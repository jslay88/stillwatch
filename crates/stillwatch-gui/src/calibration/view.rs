//! Calibration page widgets. Edits go out as [`Message`](crate::shell::Message)s.

use iced::widget::{button, column, pick_list, row, scrollable, slider, text};
use iced::{Element, Fill};

use stillwatch_core::schema::{self, Control};

use crate::edit_msg::SettingsMsg;
use crate::settings::{Editor, RegionInput};
use crate::shell::{Link, Message, Shell};

use super::draw::{self, LEGEND};
use super::{CalMsg, Calibration, ProbePace};

/// The keys the calibration sliders edit, in display order.
const TUNING: [(&str, Option<u32>); 4] = [
    ("stale.stale_percent", None),
    ("stale.persist_checks", Some(60)),
    ("stale.luma_delta_threshold", None),
    ("stale.ignore_dark_below", None),
];

/// The calibration page: interval, heatmaps, sliders, and ignore regions.
#[must_use]
pub fn page(shell: &Shell) -> Element<'_, Message> {
    let mut body = column![
        text("Calibration").size(24),
        text("Drag a rectangle on a grid to ignore those blocks. Save writes the config.").size(12),
        interval_row(shell),
        legend(),
    ]
    .spacing(10);

    if let Some(notice) = notice(shell) {
        body = body.push(text(notice).style(text::danger));
    }
    if let Some(error) = &shell.calibration.place_error {
        body = body.push(text(error).style(text::danger));
    }
    body = body.push(heatmaps(shell));
    body = body.push(tuning(shell));
    body = body.push(regions(shell));
    if let Some(error) = shell.editor.save_error() {
        body = body.push(text(error).style(text::danger));
    }
    body = body.push(save_row(&shell.editor));

    scrollable(body).height(Fill).into()
}

fn notice(shell: &Shell) -> Option<&'static str> {
    let up = matches!(shell.link, Link::Up(_));
    Calibration::idle_only_notice(up, shell.capture_known, shell.capture_backend.as_deref())
}

fn interval_row(shell: &Shell) -> Element<'_, Message> {
    let selected = shell.calibration.pace;
    let options = ProbePace::ALL.to_vec();
    row![
        text("Probe interval"),
        pick_list(options, Some(selected), |pace| {
            Message::Calibration(CalMsg::Pace(pace))
        }),
    ]
    .spacing(8)
    .into()
}

fn legend() -> Element<'static, Message> {
    let mut row = row![].spacing(12);
    for state in LEGEND {
        row = row.push(
            row![
                text("■").color(draw::swatch(state)),
                text(draw::state_name(state))
            ]
            .spacing(4),
        );
    }
    row.into()
}

fn heatmaps(shell: &Shell) -> Element<'_, Message> {
    let Some(view) = &shell.calibration.view else {
        return text("Waiting for a probe sample.").into();
    };
    if view.outputs.is_empty() {
        return text("No monitored outputs in the last sample.").into();
    }
    let regions = shell.editor.ignore_regions();
    let threshold = view.threshold_label();
    let mut maps = column![text(format!(
        "screen: {}",
        if view.stale { "STALE" } else { "not stale" }
    ))]
    .spacing(12);
    for output in &view.outputs {
        maps = maps.push(
            column![
                text(output.summary(&threshold)),
                draw::heatmap(output, shell.calibration.drag.as_ref(), regions),
            ]
            .spacing(4),
        );
    }
    maps.into()
}

fn tuning(shell: &Shell) -> Element<'_, Message> {
    let mut body = column![text("Tuning").size(20)].spacing(8);
    for (key, cap) in TUNING {
        if let Some(row) = tune_row(&shell.editor, key, cap) {
            body = body.push(row);
        }
    }
    body.into()
}

fn tune_row<'a>(
    editor: &'a Editor,
    key: &'static str,
    cap: Option<u32>,
) -> Option<Element<'a, Message>> {
    let setting = schema::find(key)?;
    let (min, max) = slider_ends(setting.control, cap, editor.number(key))?;
    let current = u16::try_from(editor.number(key).unwrap_or(u32::from(min)))
        .unwrap_or(max)
        .clamp(min, max);
    let slider = slider(
        f64::from(min)..=f64::from(max),
        f64::from(current),
        move |next| {
            Message::Calibration(CalMsg::Slider {
                key: key.to_owned(),
                value: whole(next),
            })
        },
    )
    .step(1.0);
    Some(
        column![text(format!("{}: {current}", setting.label)), slider,]
            .spacing(4)
            .into(),
    )
}

fn slider_ends(control: Control, cap: Option<u32>, current: Option<u32>) -> Option<(u16, u16)> {
    let bounds = control.bounds()?;
    let min = u16::try_from(bounds.min).ok()?;
    let upper = bounds.upper().or(cap).unwrap_or(u32::from(min));
    let upper = upper.max(current.unwrap_or(0)).min(u32::from(u16::MAX));
    let max = u16::try_from(upper).ok()?.max(min);
    Some((min, max))
}

fn whole(value: f64) -> u32 {
    let capped = value.clamp(0.0, f64::from(u16::MAX));
    let text = format!("{capped:.0}");
    text.parse().unwrap_or(0)
}

fn regions(shell: &Shell) -> Element<'_, Message> {
    let regions = shell.editor.ignore_regions();
    let mut body = column![text("Ignore regions").size(20)].spacing(6);
    if regions.is_empty() {
        body = body.push(text("None yet.").size(12));
    }
    for (index, region) in regions.iter().enumerate() {
        body = body.push(region_row(index, region, shell.calibration.editing));
    }
    body.into()
}

fn region_row(index: usize, region: &RegionInput, editing: Option<usize>) -> Element<'_, Message> {
    let label = if editing == Some(index) {
        format!(
            "{} {},{} {}x{} (redraw on the grid)",
            region.output, region.x, region.y, region.w, region.h
        )
    } else {
        format!(
            "{} {},{} {}x{}",
            region.output, region.x, region.y, region.w, region.h
        )
    };
    let mut edit = button("Edit").on_press(Message::Calibration(CalMsg::Edit(index)));
    if editing == Some(index) {
        edit = edit.style(button::primary);
    }
    row![
        text(label).width(Fill),
        edit,
        button("Delete").on_press(Message::Calibration(CalMsg::Delete(index))),
    ]
    .spacing(8)
    .into()
}

fn save_row(editor: &Editor) -> Element<'_, Message> {
    let mut save = button("Save");
    if editor.can_save() {
        save = save.on_press(Message::Settings(SettingsMsg::Save));
    }
    row![save].into()
}
