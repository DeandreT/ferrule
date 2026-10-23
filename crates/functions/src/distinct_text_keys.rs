use std::collections::BTreeSet;

use ir::Value;
use thiserror::Error;

pub const MAX_DISTINCT_TEXT_KEYS: usize = 1_000_000;
pub const MAX_DISTINCT_TEXT_KEY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DistinctTextKeyError {
    #[error("distinct text key limits exceed the hard maximum")]
    InvalidLimits,
    #[error("distinct key must be text or absent, got {0}")]
    Kind(&'static str),
    #[error("distinct text {resource} exceeds {maximum}")]
    Limit {
        resource: &'static str,
        maximum: usize,
    },
}

/// Exact ordinal membership only; callers retain first-seen item order.
pub struct DistinctTextKeys {
    keys: BTreeSet<String>,
    bytes: usize,
    max_keys: usize,
    max_bytes: usize,
}

impl Default for DistinctTextKeys {
    fn default() -> Self {
        Self {
            keys: BTreeSet::new(),
            bytes: 0,
            max_keys: MAX_DISTINCT_TEXT_KEYS,
            max_bytes: MAX_DISTINCT_TEXT_KEY_BYTES,
        }
    }
}

impl DistinctTextKeys {
    pub fn with_limits(max_keys: usize, max_bytes: usize) -> Result<Self, DistinctTextKeyError> {
        if max_keys > MAX_DISTINCT_TEXT_KEYS || max_bytes > MAX_DISTINCT_TEXT_KEY_BYTES {
            return Err(DistinctTextKeyError::InvalidLimits);
        }
        Ok(Self {
            max_keys,
            max_bytes,
            ..Self::default()
        })
    }

    /// Missing values and duplicate keys do not consume a slot. A refused key
    /// leaves prior membership and the byte budget unchanged.
    pub fn admit(&mut self, value: &Value) -> Result<bool, DistinctTextKeyError> {
        let text = match value {
            Value::Null => return Ok(false),
            Value::String(text) => text,
            other => return Err(DistinctTextKeyError::Kind(other.type_name())),
        };
        if self.keys.contains(text) {
            return Ok(false);
        }
        if self.keys.len() == self.max_keys {
            return Err(DistinctTextKeyError::Limit {
                resource: "keys",
                maximum: self.max_keys,
            });
        }
        let bytes = self
            .bytes
            .checked_add(text.len())
            .filter(|bytes| *bytes <= self.max_bytes)
            .ok_or(DistinctTextKeyError::Limit {
                resource: "key bytes",
                maximum: self.max_bytes,
            })?;
        self.keys.insert(text.clone());
        self.bytes = bytes;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_empty_whitespace_case_unicode_and_missing_identity() {
        let mut keys = DistinctTextKeys::default();
        for text in ["", " ", "A", "a", "é", "e\u{301}"] {
            assert!(keys.admit(&Value::String(text.into())).unwrap());
            assert!(!keys.admit(&Value::String(text.into())).unwrap());
        }
        assert!(!keys.admit(&Value::Null).unwrap());
        assert_eq!(
            keys.admit(&Value::Int(1)),
            Err(DistinctTextKeyError::Kind("int"))
        );
    }

    #[test]
    fn duplicate_and_failed_admissions_preserve_budgets() {
        let mut keys = DistinctTextKeys::with_limits(2, 3).unwrap();
        assert!(keys.admit(&Value::String("é".into())).unwrap());
        assert!(!keys.admit(&Value::String("é".into())).unwrap());
        assert_eq!(
            keys.admit(&Value::String("bb".into())),
            Err(DistinctTextKeyError::Limit {
                resource: "key bytes",
                maximum: 3
            })
        );
        assert!(keys.admit(&Value::String("b".into())).unwrap());
        assert_eq!(
            keys.admit(&Value::String("".into())),
            Err(DistinctTextKeyError::Limit {
                resource: "keys",
                maximum: 2
            })
        );
    }

    #[test]
    fn zero_limits_distinguish_absence_from_an_empty_key() {
        let mut none = DistinctTextKeys::with_limits(0, 0).unwrap();
        assert!(!none.admit(&Value::Null).unwrap());
        assert!(matches!(
            none.admit(&Value::String(String::new())),
            Err(DistinctTextKeyError::Limit {
                resource: "keys",
                maximum: 0
            })
        ));
        let mut empty = DistinctTextKeys::with_limits(1, 0).unwrap();
        assert!(empty.admit(&Value::String(String::new())).unwrap());
        assert!(!empty.admit(&Value::String(String::new())).unwrap());
        assert!(DistinctTextKeys::with_limits(MAX_DISTINCT_TEXT_KEYS + 1, 0).is_err());
    }
}
