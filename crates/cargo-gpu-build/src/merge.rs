//! Utilities for struct merging.

use serde::{de::DeserializeOwned, Serialize};
use serde_json::{from_value, to_value, Result, Value};

/// Merges two objects (converting them to JSON in the process),
/// but only if the incoming patch value isn't the default one.
#[inline]
pub fn merge<T>(value: &T, patch: &T) -> Result<T>
where
    T: Default + Serialize + DeserializeOwned,
{
    let json_value = to_value(value)?;
    let json_patch = to_value(patch)?;
    let json_default = to_value(T::default())?;
    let json_merged = json_merge(json_value, json_patch, &json_default);
    let merged = from_value(json_merged)?;
    Ok(merged)
}

/// Recursively merges two JSON objects,
/// but only if the incoming patch value isn't the default one.
#[inline]
#[must_use]
pub fn json_merge(mut value: Value, patch: Value, default: &Value) -> Value {
    json_merge_inner(&mut value, patch, default, None);
    value
}

/// Recursively merges two JSON objects in place,
/// but only if the incoming patch value isn't the default one.
#[inline]
pub fn json_merge_in(value: &mut Value, patch: Value, default: &Value) {
    json_merge_inner(value, patch, default, None);
}

/// Inspired by: <https://stackoverflow.com/a/47142105/575773>
fn json_merge_inner(
    old_in: &mut Value,
    new_in: Value,
    defaults: &Value,
    maybe_pointer: Option<&str>,
) {
    match (old_in, new_in) {
        (Value::Object(old), Value::Object(new)) => {
            for (key, new_value) in new {
                let pointer = maybe_pointer.unwrap_or("");
                let new_pointer = format!("{pointer}/{key}");
                let old_value = old.entry(key).or_insert(Value::Null);
                json_merge_inner(old_value, new_value, defaults, Some(&new_pointer));
            }
        }
        (old, new) => {
            let Some(default) = maybe_pointer.and_then(|pointer| defaults.pointer(pointer)) else {
                return;
            };
            if new != *default {
                *old = new;
            }
        }
    }
}
