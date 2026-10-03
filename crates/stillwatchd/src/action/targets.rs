//! Resolve `action.outputs` against the connected outputs.

use std::collections::HashSet;

use stillwatch_core::config::ActionOutputs;

/// The connector names a blank or dim should target.
///
/// `monitored` with an empty `stale.monitored_outputs` is every connected
/// output, same as `all`. An empty `connected` list means the caller does
/// not know yet; the blankers treat that as every output they can see.
#[must_use]
pub fn resolve_outputs(
    selection: ActionOutputs,
    monitored: &[String],
    connected: &[String],
) -> Vec<String> {
    match selection {
        ActionOutputs::All => connected.to_vec(),
        ActionOutputs::Monitored if monitored.is_empty() => connected.to_vec(),
        ActionOutputs::Monitored => monitored.to_vec(),
    }
}

/// Whether `targets` is a strict subset of `connected`.
///
/// `KWin` applies DPMS to every output, so a partial list must not be sent.
/// An empty `connected` list means the outputs are not known yet, and that
/// is not treated as partial.
#[must_use]
pub(super) fn is_strict_subset(targets: &[String], connected: &[String]) -> bool {
    if connected.is_empty() {
        return false;
    }
    let connected: HashSet<&str> = connected.iter().map(String::as_str).collect();
    let targets: HashSet<&str> = targets.iter().map(String::as_str).collect();
    targets.is_subset(&connected) && targets.len() < connected.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_monitored_is_every_connected_output() {
        let connected = ["HDMI-A-1".into(), "DP-1".into()];
        assert_eq!(
            resolve_outputs(ActionOutputs::Monitored, &[], &connected),
            connected
        );
        assert_eq!(
            resolve_outputs(ActionOutputs::All, &["HDMI-A-1".into()], &connected),
            connected
        );
        assert_eq!(
            resolve_outputs(ActionOutputs::Monitored, &["HDMI-A-1".into()], &connected),
            vec!["HDMI-A-1".to_owned()]
        );
        assert_eq!(
            resolve_outputs(ActionOutputs::All, &[], &[]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn strict_subset_is_partial_and_the_same_set_is_not() {
        let connected = ["HDMI-A-1".into(), "DP-1".into()];
        assert!(is_strict_subset(&["HDMI-A-1".into()], &connected));
        assert!(is_strict_subset(&[], &connected));
        assert!(!is_strict_subset(
            &["DP-1".into(), "HDMI-A-1".into()],
            &connected
        ));
        assert!(!is_strict_subset(&connected, &connected));
        assert!(!is_strict_subset(&["HDMI-A-9".into()], &connected));
        assert!(!is_strict_subset(&["HDMI-A-1".into()], &[]));
        assert!(!is_strict_subset(&[], &[]));
    }
}
