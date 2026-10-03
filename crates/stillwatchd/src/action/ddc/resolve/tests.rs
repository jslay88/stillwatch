use super::*;
use crate::action::ddc::fixtures::{PG48UQ, dell, drm_root, pg48uq_unit};
use crate::action::ddc::mock::{Call, FakeBus};

fn outputs(names: &[&str]) -> Vec<String> {
    names.iter().map(|&name| name.to_owned()).collect()
}

fn run(
    bus: &FakeBus,
    connected: &[(&str, &[u8])],
    wanted: &[&str],
) -> Result<Vec<Target>, DdcError> {
    let root = drm_root(connected);
    resolve(bus, &DrmConnectors::new(root.path()), &outputs(wanted))
}

fn target(output: &str, display: &str) -> Target {
    Target {
        output: output.into(),
        display: Ok(display.into()),
    }
}

#[test]
fn matches_outputs_to_buses_by_edid() {
    let bus = FakeBus::new()
        .with_display("i2c-2", &dell())
        .with_display("i2c-3", &PG48UQ);
    let connected: &[(&str, &[u8])] = &[("HDMI-A-1", &PG48UQ), ("DP-1", &dell())];
    let targets = run(&bus, connected, &["HDMI-A-1", "DP-1"]).unwrap();
    assert_eq!(
        targets,
        vec![target("HDMI-A-1", "i2c-3"), target("DP-1", "i2c-2")]
    );
    assert_eq!(bus.calls(), vec![Call::Scan]);
}

#[test]
fn an_empty_list_means_every_output_with_ddc() {
    let bus = FakeBus::new().with_display("i2c-3", &PG48UQ);
    let connected: &[(&str, &[u8])] = &[("HDMI-A-1", &PG48UQ), ("eDP-1", &dell())];
    assert_eq!(
        run(&bus, connected, &[]).unwrap(),
        vec![target("HDMI-A-1", "i2c-3")]
    );
}

#[test]
fn an_empty_list_fails_when_nothing_matches() {
    let bus = FakeBus::new().with_display("i2c-3", &PG48UQ);
    let error = run(&bus, &[("eDP-1", &dell())], &[]).unwrap_err();
    assert!(
        matches!(&error, DdcError::DisplayNotFound { output, reason }
            if output == "eDP-1" && reason.starts_with("no DDC/CI bus reported DEL 0xa0b1")),
        "{error}"
    );
    let none = run(&bus, &[], &[]).unwrap_err();
    assert_eq!(
        none.to_string(),
        "no DDC/CI display for output (any): no output is connected"
    );
}

#[test]
fn missing_outputs_and_displays_are_per_output_errors() {
    let bus = FakeBus::new().with_display("i2c-3", &PG48UQ);
    let connected: &[(&str, &[u8])] = &[("HDMI-A-1", &PG48UQ), ("DP-1", &dell())];
    let targets = run(&bus, connected, &["HDMI-A-1", "DP-1", "DP-9"]).unwrap();
    assert_eq!(targets[0], target("HDMI-A-1", "i2c-3"));
    assert!(matches!(
        &targets[1].display,
        Err(DdcError::DisplayNotFound { reason, .. }) if reason.contains("DEL")
    ));
    assert_eq!(
        targets[2].display.as_ref().unwrap_err().to_string(),
        "no DDC/CI display for output DP-9: it isn't connected or has no EDID"
    );
}

#[test]
fn denied_buses_explain_a_missing_display() {
    let bus = FakeBus::new().with_denied("/dev/i2c-3");
    let targets = run(&bus, &[("HDMI-A-1", &PG48UQ)], &["HDMI-A-1"]).unwrap();
    assert_eq!(
        targets[0].display,
        Err(DdcError::NoAccess {
            nodes: vec!["/dev/i2c-3".into()]
        })
    );
}

#[test]
fn identical_models_are_told_apart_by_serial() {
    let first = pg48uq_unit(1, "UNIT1");
    let second = pg48uq_unit(1, "UNIT2");
    let bus = FakeBus::new()
        .with_display("i2c-3", &first)
        .with_display("i2c-4", &second);
    let connected: &[(&str, &[u8])] = &[("HDMI-A-1", &second), ("DP-2", &first)];
    assert_eq!(
        run(&bus, connected, &[]).unwrap(),
        vec![target("DP-2", "i2c-3"), target("HDMI-A-1", "i2c-4")]
    );
}

#[test]
fn same_identity_falls_back_to_the_whole_base_block() {
    let mut other_timing = PG48UQ;
    other_timing[54] = 0x01;
    let bus = FakeBus::new()
        .with_display("i2c-3", &PG48UQ)
        .with_display("i2c-4", &other_timing);
    let targets = run(&bus, &[("HDMI-A-1", &other_timing)], &["HDMI-A-1"]).unwrap();
    assert_eq!(targets, vec![target("HDMI-A-1", "i2c-4")]);
}

#[test]
fn indistinguishable_displays_are_not_guessed() {
    let bus = FakeBus::new()
        .with_display("i2c-3", &PG48UQ)
        .with_display("i2c-4", &PG48UQ);
    let targets = run(&bus, &[("HDMI-A-1", &PG48UQ)], &["HDMI-A-1"]).unwrap();
    assert!(matches!(
        &targets[0].display,
        Err(DdcError::DisplayNotFound { reason, .. }) if reason.starts_with("2 DDC/CI buses report AUS")
    ));
}

#[test]
fn unparsable_edids_never_match() {
    let bus = FakeBus::new()
        .with_display("i2c-0", &[0u8; 128])
        .with_display("i2c-3", &PG48UQ);
    let targets = run(
        &bus,
        &[("HDMI-A-1", &PG48UQ), ("DP-1", &[1, 2, 3])],
        &["HDMI-A-1", "DP-1"],
    )
    .unwrap();
    assert_eq!(targets[0], target("HDMI-A-1", "i2c-3"));
    assert_eq!(
        targets[1].display.as_ref().unwrap_err().to_string(),
        "no DDC/CI display for output DP-1: its EDID can't be parsed: \
         EDID is 3 bytes, shorter than the 128-byte base block"
    );
}

#[test]
fn duplicate_connector_names_are_refused() {
    let root = drm_root(&[("HDMI-A-1", &PG48UQ)]);
    let other = root.path().join("card0-HDMI-A-1");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("status"), "connected").unwrap();
    std::fs::write(other.join("edid"), dell()).unwrap();
    let bus = FakeBus::new().with_display("i2c-3", &PG48UQ);
    let targets = resolve(
        &bus,
        &DrmConnectors::new(root.path()),
        &outputs(&["HDMI-A-1"]),
    )
    .unwrap();
    assert!(matches!(
        &targets[0].display,
        Err(DdcError::DisplayNotFound { reason, .. }) if reason.contains("several GPUs")
    ));
}

#[test]
fn scan_and_sysfs_failures_fail_the_whole_resolve() {
    let bus = FakeBus::new();
    bus.fail_scans(DdcError::Unavailable("no buses".into()));
    assert_eq!(
        run(&bus, &[], &["HDMI-A-1"]),
        Err(DdcError::Unavailable("no buses".into()))
    );
    let root = tempfile::tempdir().unwrap();
    let missing = DrmConnectors::new(root.path().join("missing"));
    let error = resolve(&FakeBus::new(), &missing, &[]).unwrap_err();
    assert!(
        error
            .to_string()
            .starts_with("DDC/CI unavailable: can't list DRM connectors")
    );
}
