//! Top-level keys.

use super::{Control, Section, Setting};

pub(super) const SECTION: Section = Section {
    id: "",
    title: "General",
    help: "Keys outside any section.",
    settings: &[Setting::new(
        "version",
        "Config version",
        Control::ReadOnly,
        "Config schema version. Older files are migrated in memory, and files newer than \
         this build understands are refused. Leave it alone: the settings window writes the \
         new version when it saves a migrated file.",
    )],
};
