//! The `docs/config.md` reference.

use std::fmt::Write as _;

use toml::Table;

use super::{SECTIONS, Section, Setting, default_table};
use crate::config::ConfigError;

const INTRO: &str = "\
# Configuration reference

<!-- Generated from the settings schema in crates/stillwatch-core/src/schema.
     Run `cargo xtask gen-docs` after changing it; a test fails when this file is stale. -->

Stillwatch reads `$XDG_CONFIG_HOME/stillwatch/config.toml`, normally
`~/.config/stillwatch/config.toml`. Every key is optional: missing keys and
sections take the defaults below, and unknown keys are rejected. The daemon
runs on defaults when the file doesn't exist.

- `stillwatch config init [--force] [PATH]` writes a commented copy of the defaults.
- `stillwatch config check [PATH]` validates a file without the daemon.
- Keys marked **resets detection** rebuild the capture backend and reset the
  block counters when changed. Everything else applies on reload.
";

/// The config reference: every section and key with its type, allowed
/// values, default, help text, and whether it resets detection.
///
/// # Errors
///
/// Returns [`ConfigError::Serialize`] if the default config can't be serialized.
pub fn markdown_reference() -> Result<String, ConfigError> {
    let defaults = default_table()?;
    let mut out = String::from(INTRO);
    out.push_str("\n## Sections\n\n");
    for section in SECTIONS {
        let _ = writeln!(
            out,
            "- [{}](#{}): {}",
            section.title,
            anchor(section.title),
            table_name(section)
        );
    }
    for section in SECTIONS {
        write_section(&mut out, section, &defaults);
    }
    Ok(out)
}

fn table_name(section: &Section) -> String {
    if section.id.is_empty() {
        "top-level keys".to_owned()
    } else {
        format!("`[{}]`", section.id)
    }
}

/// GitHub's heading anchor for a plain title of letters and spaces.
fn anchor(title: &str) -> String {
    title.to_lowercase().replace(' ', "-")
}

fn write_section(out: &mut String, section: &Section, defaults: &Table) {
    let _ = write!(out, "\n## {}\n\n{}\n", section.title, section.help);
    for setting in section.settings {
        write_setting(out, setting, defaults);
    }
}

fn write_setting(out: &mut String, setting: &Setting, defaults: &Table) {
    let control = &setting.control;
    let _ = write!(
        out,
        "\n### `{}`\n\n**{}.** {}\n\n- Type: {}\n",
        setting.key,
        setting.label,
        setting.help,
        control.type_name()
    );
    if let Some(value) = setting.default_in(defaults) {
        let _ = writeln!(out, "- Default: `{value}`");
    }
    if let Some(choices) = control.choices() {
        out.push_str("- Values:\n");
        for choice in choices {
            let _ = writeln!(out, "  - `{}`: {}", choice.value, choice.help);
        }
    } else if let Some(allowed) = control.allowed() {
        let _ = writeln!(out, "- Allowed: {allowed}");
    }
    if setting.resets_detection {
        out.push_str("- Resets detection\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_every_section_and_key() {
        let docs = markdown_reference().unwrap();
        for section in SECTIONS {
            assert!(docs.contains(&format!("\n## {}\n", section.title)));
            assert!(docs.contains(&format!("(#{})", anchor(section.title))));
            for setting in section.settings {
                assert!(docs.contains(&format!("\n### `{}`\n", setting.key)));
            }
        }
    }

    #[test]
    fn titles_make_plain_anchors() {
        for section in SECTIONS {
            assert!(
                section
                    .title
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == ' '),
                "{}",
                section.title
            );
        }
        assert_eq!(anchor("Stale detection"), "stale-detection");
    }

    #[test]
    fn describes_a_key_fully() {
        let docs = markdown_reference().unwrap();
        let expected = "\n### `stale.block_grid`\n\n**Block grid.** Blocks per output as \
                        `[cols, rows]`, each at most `downscale_width`. A toast or a new chat \
                        message only resets the blocks it touches.\n\n- Type: grid size (cols, rows)\n\
                        - Default: `[16, 16]`\n- Allowed: each at least 1\n- Resets detection\n";
        assert!(docs.contains(expected), "{docs}");
    }

    #[test]
    fn lists_enum_values_with_help() {
        let docs = markdown_reference().unwrap();
        assert!(docs.contains(
            "- Default: `\"dpms\"`\n- Values:\n  - `dpms`: Turn the signal off through the \
             compositor."
        ));
    }

    #[test]
    fn toc_names_each_table() {
        let docs = markdown_reference().unwrap();
        assert!(docs.contains("- [General](#general): top-level keys\n"));
        assert!(docs.contains("- [Locked session](#locked-session): `[session]`\n"));
        assert!(docs.contains("\n## Idle\n\nWhen you count as away.\n"));
    }
}
