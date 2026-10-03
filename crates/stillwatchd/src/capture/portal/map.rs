//! Match portal streams to connector names.
//!
//! A stream's position and size are in the compositor's logical coordinates,
//! the same space as `wl_output` geometry. The logical size of an output is
//! its current mode divided by the output's scale. A stream may instead
//! report the physical mode size, so both are accepted.

use crate::outputs::PlacedOutput;

/// One portal stream, without pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamGeom {
    /// `PipeWire` node id.
    pub node_id: u32,
    /// Top-left in compositor coordinates, when the portal sent it.
    pub position: Option<(i32, i32)>,
    /// Width and height in compositor coordinates, when the portal sent it.
    pub size: Option<(i32, i32)>,
}

/// A stream tied to a connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    /// `PipeWire` node id.
    pub node_id: u32,
    /// Connector name, such as `HDMI-A-1`.
    pub output: String,
}

/// Pairs each stream with at most one output.
///
/// A stream with a position matches the output whose logical rectangle is
/// that position and size (physical size is also accepted). With one stream
/// and one output, they match even when the portal omitted the rectangle.
/// Anything ambiguous is left unassigned.
#[must_use]
pub fn assign(streams: &[StreamGeom], outputs: &[PlacedOutput]) -> Vec<Assignment> {
    if streams.len() == 1 && outputs.len() == 1 {
        return vec![Assignment {
            node_id: streams[0].node_id,
            output: outputs[0].info.name.clone(),
        }];
    }
    let mut used = vec![false; outputs.len()];
    let mut assigned = Vec::new();
    for stream in streams {
        let Some(index) = unique_match(stream, outputs, &used) else {
            continue;
        };
        used[index] = true;
        assigned.push(Assignment {
            node_id: stream.node_id,
            output: outputs[index].info.name.clone(),
        });
    }
    assigned
}

fn unique_match(stream: &StreamGeom, outputs: &[PlacedOutput], used: &[bool]) -> Option<usize> {
    let hits: Vec<usize> = outputs
        .iter()
        .enumerate()
        .filter(|(index, output)| !used[*index] && matches_output(stream, output))
        .map(|(index, _)| index)
        .collect();
    (hits.len() == 1).then(|| hits[0])
}

fn matches_output(stream: &StreamGeom, output: &PlacedOutput) -> bool {
    let Some(size) = stream.size else {
        return false;
    };
    let position_ok = stream
        .position
        .is_none_or(|position| position == (output.x, output.y));
    position_ok && (size == logical_size(output) || size == physical_size(output))
}

fn logical_size(output: &PlacedOutput) -> (i32, i32) {
    let scale = u32::try_from(output.scale.max(1)).unwrap_or(1);
    let width = (output.info.width / scale).max(1);
    let height = (output.info.height / scale).max(1);
    (
        i32::try_from(width).unwrap_or(i32::MAX),
        i32::try_from(height).unwrap_or(i32::MAX),
    )
}

fn physical_size(output: &PlacedOutput) -> (i32, i32) {
    (
        i32::try_from(output.info.width).unwrap_or(i32::MAX),
        i32::try_from(output.info.height).unwrap_or(i32::MAX),
    )
}

#[cfg(test)]
mod tests;
