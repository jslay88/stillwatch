use stillwatch_core::luma::OutputInfo;

use super::{Assignment, StreamGeom, assign};
use crate::outputs::PlacedOutput;

fn placed(name: &str, x: i32, y: i32, width: u32, height: u32, scale: i32) -> PlacedOutput {
    PlacedOutput {
        info: OutputInfo::new(name, width, height),
        x,
        y,
        scale,
    }
}

fn stream(node_id: u32, position: (i32, i32), size: (i32, i32)) -> StreamGeom {
    StreamGeom {
        node_id,
        position: Some(position),
        size: Some(size),
    }
}

#[test]
fn side_by_side_monitors_follow_position() {
    let outputs = vec![
        placed("HDMI-A-1", 0, 0, 3840, 2160, 1),
        placed("DP-2", 3840, 0, 1920, 1080, 1),
    ];
    let streams = vec![
        stream(3, (3840, 0), (1920, 1080)),
        stream(2, (0, 0), (3840, 2160)),
    ];
    assert_eq!(
        assign(&streams, &outputs),
        vec![
            Assignment {
                node_id: 3,
                output: "DP-2".into(),
            },
            Assignment {
                node_id: 2,
                output: "HDMI-A-1".into(),
            },
        ]
    );
}

#[test]
fn logical_size_uses_scale() {
    let outputs = vec![placed("HDMI-A-1", 0, 0, 3840, 2160, 2)];
    let streams = vec![stream(1, (0, 0), (1920, 1080))];
    assert_eq!(assign(&streams, &outputs)[0].output, "HDMI-A-1");
}

#[test]
fn physical_size_still_matches() {
    let outputs = vec![placed("HDMI-A-1", 0, 0, 3840, 2160, 2)];
    let streams = vec![stream(1, (0, 0), (3840, 2160))];
    assert_eq!(assign(&streams, &outputs)[0].node_id, 1);
}

#[test]
fn one_stream_and_one_output_match_without_geometry() {
    let outputs = vec![placed("HDMI-A-1", 10, 20, 1920, 1080, 1)];
    let streams = vec![StreamGeom {
        node_id: 9,
        position: None,
        size: None,
    }];
    assert_eq!(
        assign(&streams, &outputs),
        vec![Assignment {
            node_id: 9,
            output: "HDMI-A-1".into(),
        }]
    );
}

#[test]
fn ambiguous_streams_stay_unassigned() {
    let outputs = vec![
        placed("HDMI-A-1", 0, 0, 1920, 1080, 1),
        placed("DP-1", 1920, 0, 1920, 1080, 1),
    ];
    let streams = vec![StreamGeom {
        node_id: 4,
        position: None,
        size: Some((1920, 1080)),
    }];
    assert_eq!(assign(&streams, &outputs), []);
}
