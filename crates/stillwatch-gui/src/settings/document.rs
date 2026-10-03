//! Edits a TOML document in place so comments and key order survive a save.
//!
//! Existing keys stay where they are. Keys the file doesn't have yet are
//! inserted in schema order, after the nearest earlier schema key that is
//! already present.

use stillwatch_core::config::Config;
use stillwatch_core::schema::{self, Section};
use toml::Value;
use toml_edit::{DocumentMut, Item, Table};

/// Applies `config` onto `document`.
///
/// An empty document starts blank. Missing keys are added in schema order.
/// Keys that already have the wanted value are left byte-for-byte, comments
/// included.
///
/// # Errors
///
/// Returns a message when `document` isn't TOML or `config` can't be serialized.
pub fn apply(document: &str, config: &Config) -> Result<String, String> {
    let mut doc = if document.trim().is_empty() {
        DocumentMut::new()
    } else {
        document
            .parse::<DocumentMut>()
            .map_err(|err| err.to_string())?
    };
    let desired = schema::table_of(config).map_err(|err| err.to_string())?;
    let root_before = key_names(doc.as_table());
    for section in schema::SECTIONS {
        fill_section(doc.as_table_mut(), section, &desired)?;
    }
    let missing = missing_keys(&root_before, &root_schema_keys());
    if !missing.is_empty() {
        let order = place(&root_before, &root_schema_keys(), &missing);
        rebuild(doc.as_table_mut(), &order);
    }
    Ok(doc.to_string())
}

fn fill_section(root: &mut Table, section: &Section, desired: &toml::Table) -> Result<(), String> {
    if section.id.is_empty() {
        let values = setting_values(section, desired)?;
        fill_values(root, &schema_names(section), &values)?;
        return Ok(());
    }
    if !root.contains_key(section.id) {
        root.insert(section.id, Item::Table(Table::new()));
    }
    let Some(table) = root.get_mut(section.id).and_then(Item::as_table_mut) else {
        return Err(format!("`{}` is not a table", section.id));
    };
    let values = setting_values(section, desired)?;
    fill_values(table, &schema_names(section), &values)
}

fn setting_values(
    section: &Section,
    desired: &toml::Table,
) -> Result<Vec<(String, Value)>, String> {
    let mut values = Vec::new();
    for setting in section.settings {
        let Some(value) = schema::lookup(desired, setting.key) else {
            return Err(format!("serialized config has no `{}`", setting.key));
        };
        values.push((setting.name().to_owned(), value.clone()));
    }
    Ok(values)
}

fn fill_values(
    table: &mut Table,
    schema_names: &[String],
    values: &[(String, Value)],
) -> Result<(), String> {
    let original = key_names(table);
    let missing = missing_keys(&original, schema_names);
    for (name, value) in values {
        set_item(table, name, value)?;
    }
    if !missing.is_empty() {
        let order = place(&original, schema_names, &missing);
        rebuild(table, &order);
    }
    Ok(())
}

fn set_item(table: &mut Table, name: &str, desired: &Value) -> Result<(), String> {
    if table
        .get(name)
        .is_some_and(|item| values_match(item, desired))
    {
        return Ok(());
    }
    let mut new_value = desired
        .to_string()
        .parse::<toml_edit::Value>()
        .map_err(|err| err.to_string())?;
    if let Some(Item::Value(existing)) = table.get(name) {
        *new_value.decor_mut() = existing.decor().clone();
    }
    match table.get_mut(name) {
        Some(slot) => *slot = Item::Value(new_value),
        None => {
            table.insert(name, Item::Value(new_value));
        }
    }
    Ok(())
}

fn values_match(item: &Item, desired: &Value) -> bool {
    item_as_toml(item).is_some_and(|current| current == *desired)
}

fn item_as_toml(item: &Item) -> Option<Value> {
    match item {
        Item::Value(value) => edit_value_to_toml(value),
        Item::ArrayOfTables(tables) => {
            let items = tables
                .iter()
                .map(table_to_toml)
                .collect::<Option<Vec<_>>>()?;
            Some(Value::Array(items))
        }
        Item::Table(_) | Item::None => None,
    }
}

fn table_to_toml(table: &Table) -> Option<Value> {
    let mut map = toml::Table::new();
    for (key, item) in table {
        map.insert(key.to_owned(), item_as_toml(item)?);
    }
    Some(Value::Table(map))
}

fn edit_value_to_toml(value: &toml_edit::Value) -> Option<Value> {
    match value {
        toml_edit::Value::String(text) => Some(Value::String(text.value().clone())),
        toml_edit::Value::Integer(number) => Some(Value::Integer(*number.value())),
        toml_edit::Value::Float(number) => Some(Value::Float(*number.value())),
        toml_edit::Value::Boolean(flag) => Some(Value::Boolean(*flag.value())),
        toml_edit::Value::Datetime(_) => None,
        toml_edit::Value::Array(array) => {
            let items = array
                .iter()
                .map(edit_value_to_toml)
                .collect::<Option<Vec<_>>>()?;
            Some(Value::Array(items))
        }
        toml_edit::Value::InlineTable(table) => {
            let mut map = toml::Table::new();
            for (key, item) in table {
                map.insert(key.to_owned(), edit_value_to_toml(item)?);
            }
            Some(Value::Table(map))
        }
    }
}

fn schema_names(section: &Section) -> Vec<String> {
    section
        .settings
        .iter()
        .map(|setting| setting.name().to_owned())
        .collect()
}

fn root_schema_keys() -> Vec<String> {
    schema::SECTIONS
        .iter()
        .flat_map(|section| {
            if section.id.is_empty() {
                schema_names(section)
            } else {
                vec![section.id.to_owned()]
            }
        })
        .collect()
}

fn key_names(table: &Table) -> Vec<String> {
    table.iter().map(|(key, _)| key.to_owned()).collect()
}

fn missing_keys(present: &[String], schema_keys: &[String]) -> Vec<String> {
    schema_keys
        .iter()
        .filter(|key| !present.iter().any(|have| have == *key))
        .cloned()
        .collect()
}

/// Inserts each missing key after its nearest earlier schema neighbor.
fn place(original: &[String], schema_keys: &[String], missing: &[String]) -> Vec<String> {
    let mut order = original.to_vec();
    for key in missing {
        let dest = destination(&order, schema_keys, key);
        order.insert(dest, key.clone());
    }
    order
}

fn destination(order: &[String], schema_keys: &[String], key: &str) -> usize {
    let Some(index) = schema_keys.iter().position(|item| item == key) else {
        return order.len();
    };
    if let Some(pred) = schema_keys[..index]
        .iter()
        .rev()
        .find(|item| order.iter().any(|have| have == *item))
    {
        return order
            .iter()
            .position(|item| item == pred)
            .map_or(order.len(), |pos| pos + 1);
    }
    if let Some(succ) = schema_keys[index + 1..]
        .iter()
        .find(|item| order.iter().any(|have| have == *item))
    {
        return order
            .iter()
            .position(|item| item == succ)
            .unwrap_or(order.len());
    }
    order.len()
}

fn rebuild(table: &mut Table, order: &[String]) {
    let names = key_names(table);
    let mut stored = Vec::new();
    for name in &names {
        if let Some(pair) = table.remove_entry(name) {
            stored.push(pair);
        }
    }
    for name in order {
        if let Some(index) = stored.iter().position(|(key, _)| key.get() == name) {
            let (key, item) = stored.remove(index);
            table.insert_formatted(&key, item);
        }
    }
    for (key, item) in stored {
        table.insert_formatted(&key, item);
    }
}
