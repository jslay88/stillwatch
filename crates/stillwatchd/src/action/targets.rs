//! Resolve `action.outputs` against the connected outputs.

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
}
