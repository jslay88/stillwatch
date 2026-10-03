//! `docs/config.md` must match what the settings schema generates.

use std::path::Path;

use stillwatch_core::schema::markdown_reference;

#[test]
fn committed_config_docs_are_up_to_date() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/config.md");
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("{}: {err}; run `cargo xtask gen-docs`", path.display()));
    let generated = markdown_reference().unwrap();
    assert!(
        committed == generated,
        "docs/config.md is out of date; run `cargo xtask gen-docs`"
    );
}
