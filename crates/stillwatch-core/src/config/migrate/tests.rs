use toml::{Table, Value};

use super::*;
use crate::config::test_support::SYNTHETIC_V0_TO_V1 as V0_TO_V1;

fn note_only(_: &mut Table) -> Vec<String> {
    vec!["checked v1 document".to_owned()]
}

const V1_TO_V2: MigrationStep = MigrationStep {
    from: 1,
    apply: note_only,
};

fn table(input: &str) -> Table {
    toml::from_str(input).unwrap()
}

#[test]
fn step_renames_key_and_records_note() {
    let input = table("version = 0\n[stale]\nthreshold_percent = 80\n");
    let (output, notes) = migrate_with(&[V0_TO_V1], input, 0, 1).unwrap();
    assert_eq!(output, table("version = 1\n[stale]\nstale_percent = 80\n"));
    assert_eq!(
        notes,
        [MigrationNote {
            from: 0,
            to: 1,
            message: "renamed `stale.threshold_percent` to `stale.stale_percent`".to_owned(),
        }]
    );
}

#[test]
fn steps_run_in_version_order_regardless_of_registry_order() {
    let input = table("[stale]\nthreshold_percent = 80\n");
    let (output, notes) = migrate_with(&[V1_TO_V2, V0_TO_V1], input, 0, 2).unwrap();
    assert_eq!(output["version"], Value::Integer(2));
    let versions: Vec<_> = notes.iter().map(|n| (n.from, n.to)).collect();
    assert_eq!(versions, [(0, 1), (1, 2)]);
}

#[test]
fn step_without_changes_adds_no_note() {
    let (output, notes) = migrate_with(&[V0_TO_V1], table("version = 0\n"), 0, 1).unwrap();
    assert_eq!(output, table("version = 1\n"));
    assert_eq!(notes, []);
}

#[test]
fn same_version_is_identity() {
    let input = table("[idle]\ninput_idle_minutes = 3\n");
    let (output, notes) = migrate_with(&[], input, 1, 1).unwrap();
    assert_eq!(
        output,
        table("version = 1\n[idle]\ninput_idle_minutes = 3\n")
    );
    assert_eq!(notes, []);
}

#[test]
fn missing_step_is_too_old() {
    let error = migrate_with(&[V1_TO_V2], Table::new(), 0, 2).unwrap_err();
    assert!(matches!(
        error,
        ConfigError::VersionTooOld {
            found: 0,
            missing: 0
        }
    ));
    let error = migrate_with(&[V0_TO_V1], Table::new(), 0, 2).unwrap_err();
    assert!(matches!(
        error,
        ConfigError::VersionTooOld {
            found: 0,
            missing: 1
        }
    ));
    assert_eq!(
        error.to_string(),
        "config version 0 is too old for this build: no migration from version 1"
    );
}

#[test]
fn newer_version_is_refused() {
    let error = migrate_with(&[], Table::new(), 3, 2).unwrap_err();
    assert!(matches!(
        error,
        ConfigError::VersionTooNew {
            found: 3,
            supported: 2
        }
    ));
}

#[test]
fn production_registry_upgrades_current_and_rejects_others() {
    let (output, notes) = migrate(Table::new(), CURRENT_VERSION).unwrap();
    assert_eq!(
        output["version"],
        Value::Integer(i64::from(CURRENT_VERSION))
    );
    assert_eq!(notes, []);
    assert!(matches!(
        migrate(Table::new(), CURRENT_VERSION + 1),
        Err(ConfigError::VersionTooNew { .. })
    ));
}

#[test]
fn production_registry_is_a_contiguous_chain_to_current() {
    let mut froms: Vec<u32> = MIGRATIONS.iter().map(|step| step.from).collect();
    froms.sort_unstable();
    let oldest = CURRENT_VERSION - u32::try_from(froms.len()).unwrap();
    let expected: Vec<u32> = (oldest..CURRENT_VERSION).collect();
    assert_eq!(froms, expected);
    assert!(migrate(Table::new(), oldest).is_ok());
}

#[test]
fn rename_key_leaves_table_alone_when_it_cannot_apply() {
    let cases = [
        ("", "no section"),
        ("stale = 3\n", "section is not a table"),
        ("[stale]\npersist_checks = 5\n", "old key absent"),
        (
            "[stale]\nthreshold_percent = 80\nstale_percent = 70\n",
            "new key already set",
        ),
    ];
    for (input, why) in cases {
        let mut doc = table(input);
        assert_eq!((V0_TO_V1.apply)(&mut doc), Vec::<String>::new(), "{why}");
        assert_eq!(doc, table(input), "{why}");
    }
}

#[test]
fn note_display() {
    let note = MigrationNote {
        from: 0,
        to: 1,
        message: "renamed `a.b` to `a.c`".to_owned(),
    };
    assert_eq!(note.to_string(), "v0 -> v1: renamed `a.b` to `a.c`");
}

#[test]
fn step_fn_is_callable_directly() {
    let mut doc = table("version = 1\n");
    assert_eq!((V1_TO_V2.apply)(&mut doc), ["checked v1 document"]);
}
