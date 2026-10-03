//! Turns form values into a [`Config`](stillwatch_core::config::Config) and back.
//!
//! Range and cross-field checks stay in [`Config::from_toml_str`](stillwatch_core::config::Config::from_toml_str).
//! A value that isn't the right shape is reported here, and the other fields are still validated.

use std::collections::BTreeMap;

use stillwatch_core::config::Config;
use stillwatch_core::schema::{self, Control, Setting};
use toml::{Table, Value};

use super::values::{FieldError, FieldValue, RegionInput};

/// The config the form currently describes.
///
/// # Errors
///
/// Returns every field problem. A shape error (text where a number belongs)
/// leaves that key at its default so the core validator can still report the
/// other fields.
pub fn config(values: &BTreeMap<String, FieldValue>) -> Result<Config, Vec<FieldError>> {
    let (table, mut errors) = encode(values)?;
    let text = match toml::to_string(&Value::Table(table)) {
        Ok(text) => text,
        Err(err) => {
            errors.push(FieldError::global(err.to_string()));
            return Err(errors);
        }
    };
    match Config::from_toml_str(&text) {
        Ok(outcome) if errors.is_empty() => Ok(outcome.config),
        Ok(_) => Err(errors),
        Err(err) => {
            let keyed = err.keyed_issues();
            if keyed.is_empty() {
                errors.push(FieldError::global(err.to_string()));
            } else {
                errors.extend(keyed.into_iter().map(|issue| FieldError {
                    key: issue.key,
                    message: issue.message,
                }));
            }
            Err(errors)
        }
    }
}

/// Every problem in `values`.
#[must_use]
pub fn issues(values: &BTreeMap<String, FieldValue>) -> Vec<FieldError> {
    config(values).err().unwrap_or_default()
}

/// One field per schema key, taken from `config`.
///
/// # Errors
///
/// Returns the first key whose stored value doesn't match its control.
pub fn fields_of(config: &Config) -> Result<BTreeMap<String, FieldValue>, String> {
    let table = schema::table_of(config).map_err(|err| err.to_string())?;
    let mut fields = BTreeMap::new();
    for setting in schema::settings() {
        let Some(value) = schema::lookup(&table, setting.key) else {
            return Err(format!("defaults have no `{}`", setting.key));
        };
        fields.insert(
            setting.key.to_owned(),
            from_toml(setting, value).map_err(|err| format!("{}: {err}", setting.key))?,
        );
    }
    Ok(fields)
}

fn encode(
    values: &BTreeMap<String, FieldValue>,
) -> Result<(Table, Vec<FieldError>), Vec<FieldError>> {
    let mut table = schema::table_of(&Config::default())
        .map_err(|err| vec![FieldError::global(err.to_string())])?;
    let mut errors = Vec::new();
    for setting in schema::settings() {
        let Some(field) = values.get(setting.key) else {
            errors.push(FieldError {
                key: setting.key.to_owned(),
                message: "missing value".to_owned(),
            });
            continue;
        };
        match to_toml(setting, field) {
            Ok(value) => {
                if let Err(err) = set_path(&mut table, setting.key, value) {
                    errors.push(FieldError {
                        key: setting.key.to_owned(),
                        message: err,
                    });
                }
            }
            Err(found) => errors.extend(found),
        }
    }
    Ok((table, errors))
}

fn set_path(table: &mut Table, key: &str, value: Value) -> Result<(), String> {
    let mut parts = key.split('.');
    let Some(first) = parts.next() else {
        return Err("empty key".to_owned());
    };
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        table.insert(first.to_owned(), value);
        return Ok(());
    }
    let child = table
        .entry(first.to_owned())
        .or_insert_with(|| Value::Table(Table::new()));
    let Some(child) = child.as_table_mut() else {
        return Err(format!("`{first}` is not a table"));
    };
    set_path(child, &rest.join("."), value)
}

fn from_toml(setting: &Setting, value: &Value) -> Result<FieldValue, String> {
    match setting.control {
        Control::Toggle => Ok(FieldValue::Bool(
            value
                .as_bool()
                .ok_or_else(|| expected("a boolean", value))?,
        )),
        Control::ReadOnly
        | Control::Int { .. }
        | Control::Percent { .. }
        | Control::Duration { .. } => Ok(FieldValue::Text(whole_number(value)?)),
        Control::Enum { .. } | Control::Text | Control::Command => Ok(FieldValue::Text(
            value
                .as_str()
                .ok_or_else(|| expected("text", value))?
                .to_owned(),
        )),
        Control::StringList
        | Control::OutputPicker
        | Control::GamepadPicker
        | Control::PlayerPicker => Ok(FieldValue::List(string_list(value)?)),
        Control::IntList { .. } => Ok(FieldValue::List(number_list(value)?)),
        Control::GridSize { .. } => {
            let numbers = number_list(value)?;
            let [cols, rows] = numbers
                .try_into()
                .map_err(|_| "expected two numbers".to_owned())?;
            Ok(FieldValue::Grid { cols, rows })
        }
        Control::RegionEditor => Ok(FieldValue::Regions(regions(value)?)),
    }
}

fn to_toml(setting: &Setting, field: &FieldValue) -> Result<Value, Vec<FieldError>> {
    let key = setting.key;
    match (&setting.control, field) {
        (Control::Toggle, FieldValue::Bool(value)) => Ok(Value::Boolean(*value)),
        (
            Control::ReadOnly
            | Control::Int { .. }
            | Control::Percent { .. }
            | Control::Duration { .. },
            FieldValue::Text(text),
        ) => parse_u32(text)
            .map(|number| Value::Integer(i64::from(number)))
            .map_err(|message| {
                vec![FieldError {
                    key: key.to_owned(),
                    message,
                }]
            }),
        (Control::Enum { choices }, FieldValue::Text(text)) => {
            if choices.iter().any(|choice| choice.value == text) {
                Ok(Value::String(text.clone()))
            } else {
                let allowed = choices
                    .iter()
                    .map(|choice| choice.value)
                    .collect::<Vec<_>>()
                    .join(" | ");
                Err(vec![FieldError {
                    key: key.to_owned(),
                    message: format!("must be one of {allowed}"),
                }])
            }
        }
        (Control::Text | Control::Command, FieldValue::Text(text)) => {
            Ok(Value::String(text.clone()))
        }
        (
            Control::StringList
            | Control::OutputPicker
            | Control::GamepadPicker
            | Control::PlayerPicker,
            FieldValue::List(items),
        ) => Ok(Value::Array(
            items.iter().cloned().map(Value::String).collect(),
        )),
        (Control::IntList { .. }, FieldValue::List(items)) => number_items(key, items),
        (Control::GridSize { .. }, FieldValue::Grid { cols, rows }) => {
            let cols = parse_u32(cols).map_err(|message| {
                vec![FieldError {
                    key: format!("{key}[0]"),
                    message,
                }]
            })?;
            let rows = parse_u32(rows).map_err(|message| {
                vec![FieldError {
                    key: format!("{key}[1]"),
                    message,
                }]
            })?;
            Ok(Value::Array(vec![
                Value::Integer(i64::from(cols)),
                Value::Integer(i64::from(rows)),
            ]))
        }
        (Control::RegionEditor, FieldValue::Regions(regions)) => region_items(key, regions),
        _ => Err(vec![FieldError {
            key: key.to_owned(),
            message: "the control doesn't match the value".to_owned(),
        }]),
    }
}

fn number_items(key: &str, items: &[String]) -> Result<Value, Vec<FieldError>> {
    let mut values = Vec::new();
    let mut errors = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match parse_u32(item) {
            Ok(number) => values.push(Value::Integer(i64::from(number))),
            Err(message) => errors.push(FieldError {
                key: format!("{key}[{index}]"),
                message,
            }),
        }
    }
    if errors.is_empty() {
        Ok(Value::Array(values))
    } else {
        Err(errors)
    }
}

fn region_items(key: &str, regions: &[RegionInput]) -> Result<Value, Vec<FieldError>> {
    let mut values = Vec::new();
    let mut errors = Vec::new();
    for (index, region) in regions.iter().enumerate() {
        match region_table(key, index, region) {
            Ok(table) => values.push(Value::Table(table)),
            Err(found) => errors.extend(found),
        }
    }
    if errors.is_empty() {
        Ok(Value::Array(values))
    } else {
        Err(errors)
    }
}

fn region_table(key: &str, index: usize, region: &RegionInput) -> Result<Table, Vec<FieldError>> {
    let mut errors = Vec::new();
    let mut table = Table::new();
    table.insert("output".to_owned(), Value::String(region.output.clone()));
    for (name, text) in [
        ("x", &region.x),
        ("y", &region.y),
        ("w", &region.w),
        ("h", &region.h),
    ] {
        match parse_u32(text) {
            Ok(number) => {
                table.insert(name.to_owned(), Value::Integer(i64::from(number)));
            }
            Err(message) => errors.push(FieldError {
                key: format!("{key}[{index}].{name}"),
                message,
            }),
        }
    }
    if errors.is_empty() {
        Ok(table)
    } else {
        Err(errors)
    }
}

fn string_list(value: &Value) -> Result<Vec<String>, String> {
    let array = value.as_array().ok_or_else(|| expected("a list", value))?;
    array
        .iter()
        .map(|item| {
            item.as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| expected("text", item))
        })
        .collect()
}

fn number_list(value: &Value) -> Result<Vec<String>, String> {
    let array = value.as_array().ok_or_else(|| expected("a list", value))?;
    array.iter().map(whole_number).collect()
}

fn regions(value: &Value) -> Result<Vec<RegionInput>, String> {
    let array = value.as_array().ok_or_else(|| expected("a list", value))?;
    array.iter().map(region_from).collect()
}

fn region_from(value: &Value) -> Result<RegionInput, String> {
    let table = value
        .as_table()
        .ok_or_else(|| expected("a region", value))?;
    let text = |name: &str| {
        table
            .get(name)
            .ok_or_else(|| format!("missing `{name}`"))
            .and_then(|item| {
                item.as_str()
                    .map(ToOwned::to_owned)
                    .or_else(|| {
                        item.as_integer()
                            .and_then(|number| u32::try_from(number).ok())
                            .map(|number| number.to_string())
                    })
                    .ok_or_else(|| format!("`{name}` is not text or a number"))
            })
    };
    Ok(RegionInput {
        output: text("output")?,
        x: text("x")?,
        y: text("y")?,
        w: text("w")?,
        h: text("h")?,
    })
}

fn whole_number(value: &Value) -> Result<String, String> {
    let Some(number) = value.as_integer() else {
        return Err(expected("a whole number", value));
    };
    u32::try_from(number)
        .map(|number| number.to_string())
        .map_err(|_| format!("must be a non-negative integer, got {number}"))
}

fn parse_u32(text: &str) -> Result<u32, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("enter a whole number".to_owned());
    }
    text.parse::<u32>()
        .map_err(|_| format!("must be a whole number, got {text}"))
}

fn expected(kind: &str, value: &Value) -> String {
    format!("expected {kind}, got {value}")
}
