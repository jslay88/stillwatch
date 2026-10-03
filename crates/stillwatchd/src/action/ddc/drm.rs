//! DRM connectors in sysfs: which outputs are connected, and their EDIDs.

use std::fs;
use std::io;
use std::path::PathBuf;

/// Where the kernel lists DRM devices and connectors.
pub(crate) const SYSFS_DRM: &str = "/sys/class/drm";

/// A connected connector and the EDID the kernel read from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Connector {
    /// Connector name without the card prefix, for example `HDMI-A-1`. This
    /// is the output name the compositor uses.
    pub name: String,
    /// Raw EDID.
    pub edid: Vec<u8>,
}

/// The connectors under a sysfs DRM root (`/sys/class/drm` outside tests).
#[derive(Debug, Clone)]
pub(crate) struct DrmConnectors {
    root: PathBuf,
}

impl DrmConnectors {
    /// Connectors under `root`.
    pub(crate) fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Every connected connector with an EDID, sorted by name.
    pub(crate) fn connected(&self) -> io::Result<Vec<Connector>> {
        let mut connectors = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str().and_then(connector_name) else {
                continue;
            };
            let dir = entry.path();
            let connected = fs::read_to_string(dir.join("status"))
                .is_ok_and(|status| status.trim() == "connected");
            let edid = if connected {
                fs::read(dir.join("edid")).unwrap_or_default()
            } else {
                Vec::new()
            };
            if !edid.is_empty() {
                connectors.push(Connector {
                    name: name.to_owned(),
                    edid,
                });
            }
        }
        connectors.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(connectors)
    }
}

/// `card1-HDMI-A-1` -> `HDMI-A-1`. The card directories themselves
/// (`card1`) and render nodes have no connector.
pub(crate) fn connector_name(entry: &str) -> Option<&str> {
    let (card, name) = entry.strip_prefix("card")?.split_once('-')?;
    let numbered = !card.is_empty() && card.bytes().all(|b| b.is_ascii_digit());
    (numbered && !name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::action::ddc::fixtures::{PG48UQ, dell};

    fn connector(root: &Path, entry: &str, status: &str, edid: &[u8]) {
        let dir = root.join(entry);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("status"), format!("{status}\n")).unwrap();
        fs::write(dir.join("edid"), edid).unwrap();
    }

    #[test]
    fn lists_connected_connectors_with_an_edid() {
        let root = tempfile::tempdir().unwrap();
        connector(root.path(), "card1-HDMI-A-1", "connected", &PG48UQ);
        connector(root.path(), "card0-DP-4", "connected", &dell());
        connector(root.path(), "card0-DP-5", "disconnected", &[]);
        connector(root.path(), "card0-HDMI-A-2", "disconnected", &dell());
        connector(root.path(), "card0-Writeback-1", "unknown", &[]);
        connector(root.path(), "card1-DP-1", "connected", &[]);
        fs::create_dir(root.path().join("card1")).unwrap();
        fs::create_dir(root.path().join("renderD128")).unwrap();
        fs::write(root.path().join("version"), "drm 1.1.0").unwrap();

        let connectors = DrmConnectors::new(root.path()).connected().unwrap();
        assert_eq!(
            connectors,
            vec![
                Connector {
                    name: "DP-4".into(),
                    edid: dell()
                },
                Connector {
                    name: "HDMI-A-1".into(),
                    edid: PG48UQ.to_vec()
                },
            ]
        );
    }

    #[test]
    fn a_missing_root_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let missing = DrmConnectors::new(root.path().join("nope"));
        assert_eq!(
            missing.connected().unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn connector_names_drop_the_card_prefix() {
        assert_eq!(connector_name("card1-HDMI-A-1"), Some("HDMI-A-1"));
        assert_eq!(connector_name("card12-eDP-1"), Some("eDP-1"));
        assert_eq!(connector_name("card1"), None);
        assert_eq!(connector_name("card-DP-1"), None);
        assert_eq!(connector_name("cardX-DP-1"), None);
        assert_eq!(connector_name("card1-"), None);
        assert_eq!(connector_name("renderD128"), None);
    }
}
