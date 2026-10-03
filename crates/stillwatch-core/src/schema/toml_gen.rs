//! The fully commented default config written by `stillwatch config init`.

use std::fmt::Write as _;

use toml::Table;

use super::{SECTIONS, Section, Setting, default_table};
use crate::config::ConfigError;

const WIDTH: usize = 79;

const HEADER: &str = "\
# Stillwatch config
#
# Every setting is listed with its default. Delete a line, or a whole section,
# to go back to the default. `stillwatch config check` validates this file, and
# docs/config.md in the Stillwatch repo describes every key.
";

/// The default config as TOML, with every section and key commented with its
/// help text, type, and allowed values.
///
/// It parses back to [`crate::config::Config::default`].
///
/// # Errors
///
/// Returns [`ConfigError::Serialize`] if the default config can't be serialized.
pub fn commented_toml() -> Result<String, ConfigError> {
    let defaults = default_table()?;
    let mut out = String::from(HEADER);
    for section in SECTIONS {
        write_section(&mut out, section, &defaults);
    }
    Ok(out)
}

fn write_section(out: &mut String, section: &Section, defaults: &Table) {
    if !section.id.is_empty() {
        out.push('\n');
        comment(out, &format!("{}: {}", section.title, section.help), "", "");
        let _ = writeln!(out, "[{}]", section.id);
    }
    for setting in section.settings {
        out.push('\n');
        write_setting(out, setting, defaults);
    }
}

fn write_setting(out: &mut String, setting: &Setting, defaults: &Table) {
    let control = &setting.control;
    let kind = match (control.choices(), control.allowed()) {
        (Some(_), Some(allowed)) => allowed,
        (None, Some(allowed)) => format!("{}, {allowed}", control.type_name()),
        (_, None) => control.type_name(),
    };
    comment(out, &format!("{} ({kind})", setting.label), "", "");
    comment(out, setting.help, "", "");
    for choice in control.choices().unwrap_or_default() {
        comment(
            out,
            &format!("{}: {}", choice.value, choice.help),
            "  ",
            "    ",
        );
    }
    if setting.resets_detection {
        comment(out, "Changing this resets detection.", "", "");
    }
    if let Some(value) = setting.default_in(defaults) {
        let _ = writeln!(out, "{} = {value}", setting.name());
    }
}

/// Appends `text` as `#` comment lines wrapped to [`WIDTH`], indenting the
/// first line by `first` and the rest by `rest`.
fn comment(out: &mut String, text: &str, first: &str, rest: &str) {
    let mut line = format!("# {first}");
    let mut empty = true;
    for word in text.split_whitespace() {
        if !empty && line.len() + 1 + word.len() > WIDTH {
            out.push_str(&line);
            out.push('\n');
            line = format!("# {rest}");
            empty = true;
        }
        if !empty {
            line.push(' ');
        }
        line.push_str(word);
        empty = false;
    }
    out.push_str(line.trim_end());
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn parses_back_to_defaults() {
        let text = commented_toml().unwrap();
        let outcome = Config::from_toml_str(&text).unwrap();
        assert_eq!(outcome.config, Config::default());
        assert_eq!(outcome.migrated_from, None);
    }

    #[test]
    fn writes_every_key_explicitly() {
        let written: Table = toml::from_str(&commented_toml().unwrap()).unwrap();
        assert_eq!(written, default_table().unwrap());
    }

    #[test]
    fn lines_fit_the_width() {
        for line in commented_toml().unwrap().lines() {
            if line.starts_with('#') && line.contains(' ') {
                let longest_word = line.split_whitespace().map(str::len).max().unwrap();
                assert!(line.len() <= WIDTH || longest_word + 2 > WIDTH, "{line}");
            }
        }
    }

    #[test]
    fn documents_choices_and_resets() {
        let text = commented_toml().unwrap();
        assert!(text.contains("# Capture backend (auto | kwin | portal)\n"));
        assert!(text.contains("#   overlay: Cover each output with a black surface."));
        assert!(text.contains("# Changing this resets detection.\nbackend = \"auto\"\n"));
        assert!(text.contains("# Stale threshold (percent, 1 to 100)\n"));
        assert!(text.contains("# Gamepad counts as input (boolean)\n"));
        assert!(text.contains("block_grid = [16, 16]\n"));
    }

    #[test]
    fn comment_wraps_and_indents() {
        let mut out = String::new();
        comment(&mut out, &"word ".repeat(20), "  ", "    ");
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("#   word"));
        assert!(lines[1].starts_with("#     word"));
        assert!(lines.iter().all(|line| line.len() <= WIDTH));
    }

    #[test]
    fn comment_keeps_overlong_words_whole() {
        let mut out = String::new();
        let long = "x".repeat(WIDTH + 5);
        comment(&mut out, &format!("short {long} tail"), "", "");
        assert_eq!(out, format!("# short\n# {long}\n# tail\n"));
    }
}
