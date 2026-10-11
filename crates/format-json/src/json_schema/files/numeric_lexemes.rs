//! Exact numeric-bound lexemes tied to one immutable loaded resource bundle.
//!
//! The ordinary Value parser remains authoritative for JSON syntax, overflow
//! and recursion. Sparse sidecars retain only checked integral candidates;
//! they never replace annotation/data values in the original document. Schema
//! views consult them only at the existing parser's actual schema entries and
//! reference hops, without resolving any additional references.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Deref;

use serde_json::{Number, Value, value::RawValue};

use super::{JsonPosition, MAX_JSON_DEPTH, schema_keyword_position};
use crate::JsonFormatError;

#[derive(Debug, Default)]
pub(super) struct Lexemes {
    numbers: BTreeMap<String, Number>,
    object: BTreeMap<String, Lexemes>,
    array: BTreeMap<usize, Lexemes>,
}

impl Lexemes {
    fn is_empty(&self) -> bool {
        self.numbers.is_empty() && self.object.is_empty() && self.array.is_empty()
    }

    pub(super) fn bundle_external(&mut self, external: BTreeMap<String, Lexemes>) {
        self.object
            .entry("$defs".to_string())
            .or_default()
            .object
            .insert(
                super::EXTERNAL_DEFS_KEY.to_string(),
                Lexemes {
                    object: external,
                    ..Self::default()
                },
            );
    }

    fn bind(self, value: &Value, patches: &mut BTreeMap<usize, BTreeMap<String, Number>>) {
        if !self.numbers.is_empty() {
            patches.insert(value as *const Value as usize, self.numbers);
        }
        if let Value::Object(object) = value {
            for (key, lexemes) in self.object {
                if let Some(child) = object.get(&key) {
                    lexemes.bind(child, patches);
                }
            }
        }
        if let Value::Array(items) = value {
            for (index, lexemes) in self.array {
                if let Some(child) = items.get(index) {
                    lexemes.bind(child, patches);
                }
            }
        }
    }
}

pub(super) struct ParsedDocument {
    pub(super) value: Value,
    pub(super) lexemes: Lexemes,
}

#[derive(Debug)]
pub(in crate::json_schema) struct LoadedSchema {
    // Boxing keeps the root address stable across moves. Descendant addresses
    // stay stable because this original Value is never mutated after binding.
    value: Box<Value>,
    patches: BTreeMap<usize, BTreeMap<String, Number>>,
}

impl Deref for LoadedSchema {
    type Target = Value;

    fn deref(&self) -> &Value {
        &self.value
    }
}

impl LoadedSchema {
    pub(super) fn new(value: Value, lexemes: Lexemes) -> Self {
        let value = Box::new(value);
        let mut patches = BTreeMap::new();
        // Addresses are lookup keys only; no pointer is dereferenced and there
        // is no unsafe, thread-local or global state.
        lexemes.bind(&value, &mut patches);
        Self { value, patches }
    }

    #[cfg(test)]
    pub(in crate::json_schema) fn from_value(value: Value) -> Self {
        Self::new(value, Lexemes::default())
    }

    pub(in crate::json_schema) fn schema_view<'a>(
        &self,
        schema: &'a Value,
    ) -> Result<Cow<'a, Value>, JsonFormatError> {
        if !self.walk(schema, None, JsonPosition::Schema, 0)? {
            return Ok(Cow::Borrowed(schema));
        }
        let mut normalized = schema.clone();
        self.walk(schema, Some(&mut normalized), JsonPosition::Schema, 0)?;
        Ok(Cow::Owned(normalized))
    }

    fn walk(
        &self,
        original: &Value,
        mut output: Option<&mut Value>,
        position: JsonPosition,
        depth: usize,
    ) -> Result<bool, JsonFormatError> {
        check_depth(depth)?;
        let mut changed = false;
        match position {
            JsonPosition::Schema
            | JsonPosition::SchemaMap
            | JsonPosition::LegacyDependenciesMap => {
                let Value::Object(object) = original else {
                    return Ok(false);
                };
                if matches!(position, JsonPosition::Schema)
                    && let Some(numbers) = self.patches.get(&(original as *const Value as usize))
                {
                    changed = true;
                    if let Some(output) = output.as_deref_mut().and_then(Value::as_object_mut) {
                        for (key, number) in numbers {
                            output.insert(key.clone(), Value::Number(number.clone()));
                        }
                    }
                }
                for (keyword, child) in object {
                    let position = match position {
                        JsonPosition::Schema => schema_keyword_position(keyword),
                        JsonPosition::SchemaMap => JsonPosition::Schema,
                        JsonPosition::LegacyDependenciesMap => {
                            if child.is_object() || child.is_boolean() {
                                JsonPosition::Schema
                            } else {
                                JsonPosition::Opaque
                            }
                        }
                        _ => unreachable!(),
                    };
                    let out = output.as_deref_mut().and_then(|v| v.get_mut(keyword));
                    changed |= self.walk(child, out, position, depth + 1)?;
                }
            }
            JsonPosition::SchemaArray => {
                let Value::Array(items) = original else {
                    return Ok(false);
                };
                for (index, child) in items.iter().enumerate() {
                    let out = output.as_deref_mut().and_then(|v| v.get_mut(index));
                    changed |= self.walk(child, out, JsonPosition::Schema, depth + 1)?;
                }
            }
            JsonPosition::Items => {
                let position = if original.is_array() {
                    JsonPosition::SchemaArray
                } else if original.is_object() || original.is_boolean() {
                    JsonPosition::Schema
                } else {
                    JsonPosition::Opaque
                };
                changed |= self.walk(original, output, position, depth)?;
            }
            JsonPosition::Opaque => {}
        }
        Ok(changed)
    }
}

pub(super) fn parse(bytes: &[u8]) -> Result<ParsedDocument, JsonFormatError> {
    let value = serde_json::from_slice(bytes)?;
    let raw = serde_json::from_slice::<&RawValue>(bytes)?;
    let lexemes = collect(&value, raw, 0)?;
    Ok(ParsedDocument { value, lexemes })
}

fn check_depth(depth: usize) -> Result<(), JsonFormatError> {
    if depth > MAX_JSON_DEPTH {
        return Err(JsonFormatError::SchemaResourceLimit {
            kind: "JSON nesting depth",
            limit: MAX_JSON_DEPTH,
        });
    }
    Ok(())
}

fn collect(value: &Value, raw: &RawValue, depth: usize) -> Result<Lexemes, JsonFormatError> {
    check_depth(depth)?;
    let mut result = Lexemes::default();
    match value {
        Value::Object(object) => {
            // Decoded keys and last duplicate values agree with Value. Its
            // original first-insertion property order is not modified.
            let entries: BTreeMap<String, &RawValue> = serde_json::from_str(raw.get())?;
            for (keyword, child) in object {
                let raw = entries.get(keyword).ok_or_else(|| {
                    <serde_json::Error as serde::de::Error>::custom(
                        "schema lexeme key is absent from the original object",
                    )
                })?;
                if is_bound(keyword)
                    && child.as_number().is_some_and(Number::is_f64)
                    && let Some(number) = exact_integer(raw.get())
                {
                    result.numbers.insert(keyword.clone(), number);
                }
                let lexemes = collect(child, raw, depth + 1)?;
                if !lexemes.is_empty() {
                    result.object.insert(keyword.clone(), lexemes);
                }
            }
        }
        Value::Array(items) => {
            let originals: Vec<&RawValue> = serde_json::from_str(raw.get())?;
            if items.len() != originals.len() {
                return Err(<serde_json::Error as serde::de::Error>::custom(
                    "schema lexeme array differs from the original array",
                )
                .into());
            }
            for (index, (child, raw)) in items.iter().zip(originals).enumerate() {
                let lexemes = collect(child, raw, depth + 1)?;
                if !lexemes.is_empty() {
                    result.array.insert(index, lexemes);
                }
            }
        }
        _ => {}
    }
    Ok(result)
}

fn is_bound(keyword: &str) -> bool {
    matches!(
        keyword,
        "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum"
    )
}

// The lexeme has already passed the ordinary JSON parser. Work is linear in
// its length, accumulation is checked u64 arithmetic, and zero padding is at
// most 19 digits. Huge exponents never expand into a string or allocation.
fn exact_integer(lexeme: &str) -> Option<Number> {
    let lexeme = lexeme.trim();
    let negative = lexeme.starts_with('-');
    let unsigned = lexeme.strip_prefix('-').unwrap_or(lexeme);
    let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
    let fraction_digits = mantissa.split_once('.').map_or(0, |(_, tail)| tail.len());
    let mut significant_digits = 0_usize;
    let mut trailing_zeroes = 0_usize;
    let mut total_digits = 0_usize;
    for digit in mantissa.bytes().filter(|digit| *digit != b'.') {
        total_digits += 1;
        if significant_digits != 0 || digit != b'0' {
            significant_digits += 1;
        }
        if digit == b'0' {
            trailing_zeroes += 1;
        } else {
            trailing_zeroes = 0;
        }
    }
    if significant_digits == 0 {
        return Some(Number::from(0));
    }
    let shift = exponent
        .parse::<i64>()
        .ok()?
        .checked_sub(i64::try_from(fraction_digits).ok()?)?;
    let integer_digits = i64::try_from(significant_digits).ok()?.checked_add(shift)?;
    if !(1..=20).contains(&integer_digits) {
        return None;
    }
    let removed = if shift < 0 {
        usize::try_from(shift.checked_neg()?).ok()?
    } else {
        0
    };
    if removed > trailing_zeroes {
        return None;
    }
    let retained = total_digits.checked_sub(removed)?;
    let mut magnitude = 0_u64;
    for digit in mantissa
        .bytes()
        .filter(|digit| *digit != b'.')
        .take(retained)
    {
        magnitude = magnitude
            .checked_mul(10)?
            .checked_add(u64::from(digit - b'0'))?;
    }
    for _ in 0..shift.max(0) {
        magnitude = magnitude.checked_mul(10)?;
    }
    if negative {
        let signed = if magnitude == 1_u64 << 63 {
            i64::MIN
        } else {
            -i64::try_from(magnitude).ok()?
        };
        Some(Number::from(signed))
    } else {
        Some(Number::from(magnitude))
    }
}

#[cfg(test)]
mod tests;
