//! Which DDC/CI display drives which output, matched by EDID identity.
//!
//! The capabilities string isn't used: the PG48UQ fails to return one.

use super::DdcError;
use super::drm::{Connector, DrmConnectors};
use super::edid;
use super::transport::{DdcDisplay, DdcTransport, Scan};

/// One requested output and the display it maps to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// Connector name.
    pub output: String,
    /// The transport's display handle, or why there is none.
    pub display: Result<String, DdcError>,
}

/// Maps `outputs` to DDC displays with one bus scan. An empty list means
/// every connected output that has a DDC display; outputs without one (a
/// laptop panel, say) are skipped unless none has one.
///
/// # Errors
///
/// Fails as a whole only if sysfs can't be read, the scan fails, or an empty
/// list matched nothing. Per-output problems are in each [`Target`].
pub(crate) fn resolve(
    transport: &dyn DdcTransport,
    drm: &DrmConnectors,
    outputs: &[String],
) -> Result<Vec<Target>, DdcError> {
    let connectors = drm
        .connected()
        .map_err(|e| DdcError::Unavailable(format!("can't list DRM connectors: {e}")))?;
    let scan = transport.scan()?;
    if outputs.is_empty() {
        return every(&connectors, &scan);
    }
    Ok(outputs
        .iter()
        .map(|output| Target {
            output: output.clone(),
            display: named(output, &connectors, &scan),
        })
        .collect())
}

fn every(connectors: &[Connector], scan: &Scan) -> Result<Vec<Target>, DdcError> {
    let mut first_error = None;
    let targets: Vec<Target> = connectors
        .iter()
        .filter_map(|connector| match find(connector, scan) {
            Ok(display) => Some(Target {
                output: connector.name.clone(),
                display: Ok(display.id.clone()),
            }),
            Err(error) => {
                tracing::debug!(output = connector.name, %error, "skipping output");
                first_error.get_or_insert(error);
                None
            }
        })
        .collect();
    if targets.is_empty() {
        return Err(first_error.unwrap_or_else(|| not_found("(any)", "no output is connected")));
    }
    Ok(targets)
}

fn named(output: &str, connectors: &[Connector], scan: &Scan) -> Result<String, DdcError> {
    let mut matching = connectors.iter().filter(|c| c.name == output);
    let connector = matching
        .next()
        .ok_or_else(|| not_found(output, "it isn't connected or has no EDID"))?;
    if matching.next().is_some() {
        return Err(not_found(
            output,
            "several GPUs have a connector by that name",
        ));
    }
    find(connector, scan).map(|display| display.id.clone())
}

/// The scanned display whose EDID identifies the same unit as `connector`'s.
/// Identical models with unset serials fall back to comparing the whole base
/// block.
pub(crate) fn find<'a>(connector: &Connector, scan: &'a Scan) -> Result<&'a DdcDisplay, DdcError> {
    let wanted = edid::parse(&connector.edid)
        .map_err(|e| not_found(&connector.name, &format!("its EDID can't be parsed: {e}")))?;
    let same: Vec<&DdcDisplay> = scan
        .displays
        .iter()
        .filter(|d| edid::parse(&d.edid).is_ok_and(|identity| identity.same_unit(&wanted)))
        .collect();
    match same.as_slice() {
        [display] => Ok(display),
        [] if !scan.denied.is_empty() => Err(DdcError::NoAccess {
            nodes: scan.denied.clone(),
        }),
        [] => Err(not_found(
            &connector.name,
            &format!("no DDC/CI bus reported {wanted}"),
        )),
        several => {
            let block = edid::base_block(&connector.edid);
            let exact: Vec<&DdcDisplay> = several
                .iter()
                .copied()
                .filter(|d| edid::base_block(&d.edid) == block)
                .collect();
            match exact.as_slice() {
                [display] => Ok(display),
                _ => Err(not_found(
                    &connector.name,
                    &format!(
                        "{} DDC/CI buses report {wanted} and can't be told apart",
                        several.len()
                    ),
                )),
            }
        }
    }
}

fn not_found(output: &str, reason: &str) -> DdcError {
    DdcError::DisplayNotFound {
        output: output.to_owned(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests;
