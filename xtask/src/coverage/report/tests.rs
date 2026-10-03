use super::{CrateDir, Lines, Thresholds, Totals, Violation, aggregate, evaluate, render};

const THRESHOLDS: Thresholds<'static> = Thresholds {
    workspace: 80,
    crates: &[("stillwatch-core", 90)],
};

fn crates() -> Vec<CrateDir> {
    ["stillwatch-core", "stillwatch-ipc", "stillwatchd"]
        .into_iter()
        .map(|name| CrateDir {
            name: name.to_owned(),
            dir: format!("/ws/crates/{name}").into(),
        })
        .collect()
}

fn file(name: &str, count: u64, covered: u64) -> String {
    format!(
        r#"{{"filename": "{name}", "summary": {{
            "lines": {{"count": {count}, "covered": {covered}, "percent": 0}},
            "functions": {{"count": 1, "covered": 1, "percent": 100}}
        }}}}"#
    )
}

fn export(files: &[String]) -> String {
    format!(
        r#"{{"type": "llvm.coverage.json.export", "version": "2.0.1",
            "data": [{{"files": [{}], "totals": {{}}}}]}}"#,
        files.join(",")
    )
}

fn lines(count: u64, covered: u64) -> Lines {
    Lines { count, covered }
}

#[test]
fn aggregates_files_per_crate() {
    let json = export(&[
        file("/ws/crates/stillwatch-core/src/lib.rs", 100, 95),
        file("/ws/crates/stillwatch-core/src/config/mod.rs", 50, 40),
        file("/ws/crates/stillwatch-ipc/src/lib.rs", 10, 10),
        file("/ws/crates/stillwatch-ipcx/src/lib.rs", 5, 0),
    ]);
    let totals = aggregate(&json, &crates()).unwrap();
    assert_eq!(totals.workspace, lines(165, 145));
    assert_eq!(totals.crates["stillwatch-core"], lines(150, 135));
    assert_eq!(totals.crates["stillwatch-ipc"], lines(10, 10));
    assert_eq!(totals.crates["stillwatchd"], lines(0, 0));
}

#[test]
fn nested_crate_dirs_pick_the_deepest() {
    let crates = vec![
        CrateDir {
            name: "outer".into(),
            dir: "/ws".into(),
        },
        CrateDir {
            name: "inner".into(),
            dir: "/ws/inner".into(),
        },
    ];
    let json = export(&[
        file("/ws/inner/src/lib.rs", 4, 2),
        file("/ws/src/lib.rs", 2, 2),
    ]);
    let totals = aggregate(&json, &crates).unwrap();
    assert_eq!(totals.crates["inner"], lines(4, 2));
    assert_eq!(totals.crates["outer"], lines(2, 2));
}

#[test]
fn passes_at_exact_thresholds() {
    let json = export(&[
        file("/ws/crates/stillwatch-core/src/lib.rs", 100, 90),
        file("/ws/crates/stillwatchd/src/run.rs", 100, 70),
    ]);
    let totals = aggregate(&json, &crates()).unwrap();
    assert_eq!(evaluate(&totals, THRESHOLDS), []);
}

#[test]
fn reports_each_scope_below_threshold() {
    let json = export(&[
        file("/ws/crates/stillwatch-core/src/lib.rs", 1000, 899),
        file("/ws/crates/stillwatchd/src/run.rs", 1000, 700),
    ]);
    let totals = aggregate(&json, &crates()).unwrap();
    assert_eq!(
        evaluate(&totals, THRESHOLDS),
        [
            Violation {
                scope: "workspace".into(),
                lines: lines(2000, 1599),
                min: 80
            },
            Violation {
                scope: "stillwatch-core".into(),
                lines: lines(1000, 899),
                min: 90
            },
        ]
    );
}

#[test]
fn nothing_instrumented_passes() {
    let totals = aggregate(&export(&[]), &crates()).unwrap();
    assert_eq!(totals.workspace, Lines::default());
    assert_eq!(evaluate(&totals, THRESHOLDS), []);
    assert_eq!(evaluate(&Totals::default(), THRESHOLDS), []);
}

#[test]
fn rejects_malformed_json() {
    assert!(aggregate("{}", &crates()).is_err());
    assert!(aggregate("not json", &crates()).is_err());
}

#[test]
fn percent_formatting() {
    assert_eq!(lines(0, 0).percent(), "-");
    assert_eq!(lines(3, 2).percent(), "66.6%");
    assert_eq!(lines(8, 8).percent(), "100.0%");
}

#[test]
fn renders_a_row_per_crate_and_workspace() {
    let json = export(&[file("/ws/crates/stillwatch-core/src/lib.rs", 8, 6)]);
    let table = render(&aggregate(&json, &crates()).unwrap());
    assert_eq!(
        table,
        "\
stillwatch-core          6/8   75.0%
stillwatch-ipc           0/0       -
stillwatchd              0/0       -
workspace                6/8   75.0%
"
    );
}
