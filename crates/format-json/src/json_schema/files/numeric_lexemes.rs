//! Preserve exact integral schema endpoints before `Value` rounds decimal tokens.

use std::fmt;

use serde::Deserialize;
use serde::de::{MapAccess, Visitor};
use serde_json::value::RawValue;

use super::{JsonPosition, MAX_JSON_DEPTH, schema_keyword_position};
use crate::JsonFormatError;

pub(super) fn decode(bytes: &[u8]) -> Result<serde_json::Value, JsonFormatError> {
    let normalized = normalize(bytes)?;
    // Retaining every raw entry lets this ordinary decoder keep its duplicate-key
    // behavior and reject invalid numbers even in earlier duplicate entries.
    Ok(serde_json::from_str(&normalized)?)
}

fn normalize(bytes: &[u8]) -> Result<String, JsonFormatError> {
    let raw: &RawValue = serde_json::from_slice(bytes)?;
    let mut out = String::with_capacity(bytes.len());
    rewrite(raw, JsonPosition::Schema, 0, &mut out)?;
    Ok(out)
}

// A map would discard earlier duplicate values before ordinary JSON parsing.
// Borrow the raw values so recursive schema walks do not copy each subtree.
struct RawObject<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for RawObject<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ObjectVisitor;
        impl<'de> Visitor<'de> for ObjectVisitor {
            type Value = RawObject<'de>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some(entry) = access.next_entry::<String, &'de RawValue>()? {
                    entries.push(entry);
                }
                Ok(RawObject(entries))
            }
        }
        deserializer.deserialize_map(ObjectVisitor)
    }
}

fn rewrite(
    raw: &RawValue,
    position: JsonPosition,
    depth: usize,
    out: &mut String,
) -> Result<(), JsonFormatError> {
    if depth > MAX_JSON_DEPTH {
        return Err(JsonFormatError::SchemaResourceLimit {
            kind: "JSON nesting depth",
            limit: MAX_JSON_DEPTH,
        });
    }
    let source = raw.get();
    let first = source.as_bytes().first().copied();
    let position = match position {
        JsonPosition::Items => match first {
            Some(b'[') => JsonPosition::SchemaArray,
            Some(b'{' | b't' | b'f') => JsonPosition::Schema,
            _ => JsonPosition::Opaque,
        },
        position => position,
    };
    match (position, first) {
        (
            JsonPosition::Schema | JsonPosition::SchemaMap | JsonPosition::LegacyDependenciesMap,
            Some(b'{'),
        ) => {
            let RawObject(entries) = serde_json::from_str(source)?;
            out.push('{');
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index != 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(&key)?);
                out.push(':');
                if matches!(position, JsonPosition::Schema)
                    && matches!(
                        key.as_str(),
                        "minimum" | "maximum" | "exclusiveMinimum" | "exclusiveMaximum"
                    )
                    && let Some(integer) = exact_integer(value.get())
                {
                    out.push_str(&integer);
                    continue;
                }
                let child_position = match position {
                    JsonPosition::Schema => schema_keyword_position(&key),
                    JsonPosition::SchemaMap => JsonPosition::Schema,
                    JsonPosition::LegacyDependenciesMap => {
                        if matches!(value.get().as_bytes().first(), Some(b'{' | b't' | b'f')) {
                            JsonPosition::Schema
                        } else {
                            JsonPosition::Opaque
                        }
                    }
                    _ => unreachable!(),
                };
                rewrite(value, child_position, depth + 1, out)?;
            }
            out.push('}');
        }
        (JsonPosition::SchemaArray, Some(b'[')) => {
            let entries: Vec<&RawValue> = serde_json::from_str(source)?;
            out.push('[');
            for (index, value) in entries.into_iter().enumerate() {
                if index != 0 {
                    out.push(',');
                }
                rewrite(value, JsonPosition::Schema, depth + 1, out)?;
            }
            out.push(']');
        }
        _ => out.push_str(source),
    }
    Ok(())
}

/// Prove integrality and the JSON integer-token domain using decimal digits only.
/// Unproved values remain raw for the existing decoder and range admission.
fn exact_integer(raw: &str) -> Option<String> {
    let (negative, source) = raw
        .strip_prefix('-')
        .map_or((false, raw), |source| (true, source));
    if !source.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let (mantissa, exponent) = source.find(['e', 'E']).map_or((source, None), |index| {
        (&source[..index], Some(&source[index + 1..]))
    });
    let fraction_digits = mantissa
        .find('.')
        .map_or(0, |index| mantissa.len() - index - 1);
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Some("0".into());
    }
    let exponent = exponent.map_or(Some(0_i64), |exponent| exponent.parse().ok())?;
    let scale = exponent.checked_sub(i64::try_from(fraction_digits).ok()?)?;
    let (digits, appended_zeros) = if scale < 0 {
        let removed = usize::try_from(scale.checked_neg()?).ok()?;
        let trailing_zeros = digits
            .bytes()
            .rev()
            .take_while(|byte| *byte == b'0')
            .count();
        if removed > trailing_zeros {
            return None;
        }
        (&digits[..digits.len() - removed], 0)
    } else {
        (digits, usize::try_from(scale).ok()?)
    };
    // No huge power, exponent-sized allocation, or rounded floating conversion.
    let length = digits.len().checked_add(appended_zeros)?;
    let limit = if negative {
        "9223372036854775808"
    } else {
        "18446744073709551615"
    };
    if length > limit.len() {
        return None;
    }
    let mut integer = String::with_capacity(length);
    integer.push_str(digits);
    integer.extend(std::iter::repeat_n('0', appended_zeros));
    if length == limit.len() && integer.as_str() > limit {
        return None;
    }
    if negative {
        integer.insert(0, '-');
    }
    Some(integer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(label: &str, original: &impl fmt::Debug) -> Result<(), Box<dyn std::error::Error>> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let parent = std::env::var_os("FERRULE_LEXICAL_BOUNDS_EVIDENCE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        std::fs::write(
            parent.join(format!(
                "ferrule-lexemes-{label}-{}-{stamp}.txt",
                std::process::id()
            )),
            format!("{original:#?}\n"),
        )?;
        Ok(())
    }

    #[test]
    fn lexical_bound_exact_digit_oracles() -> Result<(), Box<dyn std::error::Error>> {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/lexical_numeric_bounds/lexeme-cases.json"
        ))?;
        let outcomes = rows
            .iter()
            .map(|row| (row, exact_integer(row["raw"].as_str().unwrap())))
            .collect::<Vec<_>>();
        record("digits", &outcomes)?;
        assert!(
            outcomes
                .iter()
                .all(|(row, actual)| actual.as_deref() == row["canonical_integer"].as_str())
        );
        Ok(())
    }

    #[test]
    fn lexical_bound_schema_positions_and_opaque_oracles() -> Result<(), Box<dyn std::error::Error>>
    {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tests/fixtures/lexical_numeric_bounds/position-cases.json"
        ))?;
        let outcomes = rows
            .iter()
            .map(|row| {
                (
                    row,
                    normalize(row["raw_schema"].as_str().unwrap().as_bytes()),
                )
            })
            .collect::<Vec<_>>();
        record("positions", &outcomes)?;
        assert!(outcomes.iter().all(|(row, actual)| matches!(actual, Ok(actual) if row["expected_normalized_json"] == actual.as_str())));
        Ok(())
    }
}
