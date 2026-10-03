//! The form's copy of each schema value, before it is a [`Config`](stillwatch_core::config::Config).

use stillwatch_core::schema;

/// One problem, keyed the same way validation issues are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// Dotted path, or empty when the problem is about the whole file.
    pub key: String,
    /// What is wrong.
    pub message: String,
}

impl FieldError {
    /// A problem that isn't tied to one setting.
    #[must_use]
    pub fn global(message: impl Into<String>) -> Self {
        Self {
            key: String::new(),
            message: message.into(),
        }
    }
}

/// One ignore-region as the text the form is editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionInput {
    /// Output connector name.
    pub output: String,
    /// Left edge, as text.
    pub x: String,
    /// Top edge, as text.
    pub y: String,
    /// Width, as text.
    pub w: String,
    /// Height, as text.
    pub h: String,
}

impl RegionInput {
    /// A new region the user still has to fill in.
    #[must_use]
    pub fn blank() -> Self {
        Self {
            output: String::new(),
            x: "0".to_owned(),
            y: "0".to_owned(),
            w: "1".to_owned(),
            h: "1".to_owned(),
        }
    }

    pub(crate) fn set(&mut self, part: crate::edit_msg::RegionPart, value: String) {
        match part {
            crate::edit_msg::RegionPart::Output => self.output = value,
            crate::edit_msg::RegionPart::X => self.x = value,
            crate::edit_msg::RegionPart::Y => self.y = value,
            crate::edit_msg::RegionPart::W => self.w = value,
            crate::edit_msg::RegionPart::H => self.h = value,
        }
    }
}

/// The value a control is editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    /// A toggle.
    Bool(bool),
    /// Text, a number, an enum value, or a read-only number.
    Text(String),
    /// A list of strings or whole numbers, stored as text.
    List(Vec<String>),
    /// `[cols, rows]`.
    Grid {
        /// Column count, as text.
        cols: String,
        /// Row count, as text.
        rows: String,
    },
    /// Ignore-regions.
    Regions(Vec<RegionInput>),
}

/// Messages for `setting_key`, including indexed paths such as `key[0].w`.
#[must_use]
pub fn field_messages(issues: &[FieldError], setting_key: &str) -> Vec<String> {
    issues
        .iter()
        .filter_map(|issue| message_for(issue, setting_key))
        .collect()
}

/// Problems that don't belong on a setting.
#[must_use]
pub fn global_messages(issues: &[FieldError]) -> Vec<String> {
    issues
        .iter()
        .filter(|issue| issue.key.is_empty() || schema::find(&issue.key).is_none())
        .map(|issue| {
            if issue.key.is_empty() {
                issue.message.clone()
            } else {
                format!("{}: {}", issue.key, issue.message)
            }
        })
        .collect()
}

/// Splits daemon `key: message` lines into field errors and everything else.
#[must_use]
pub fn external_issues(lines: &[String]) -> (Vec<FieldError>, Vec<String>) {
    let mut keyed = Vec::new();
    let mut other = Vec::new();
    for line in lines {
        match line.split_once(": ") {
            Some((key, message)) if schema::find(key).is_some() => keyed.push(FieldError {
                key: key.to_owned(),
                message: message.to_owned(),
            }),
            _ => other.push(line.clone()),
        }
    }
    (keyed, other)
}

fn message_for(issue: &FieldError, setting_key: &str) -> Option<String> {
    if issue.key == setting_key {
        return Some(issue.message.clone());
    }
    let setting = schema::find(&issue.key)?;
    if setting.key != setting_key {
        return None;
    }
    let tail = issue
        .key
        .strip_prefix(setting_key)
        .unwrap_or(issue.key.as_str());
    Some(format!("{tail}: {}", issue.message))
}
