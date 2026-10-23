use super::ScopeContext;
use crate::{RuntimeError, Value};

pub use functions::{MAX_DISTINCT_TEXT_KEY_BYTES, MAX_DISTINCT_TEXT_KEYS};

impl ScopeContext<'_> {
    /// Complete key selection precedes every mapped expression. The selected
    /// contexts retain the first item's positions, source identity and clocks.
    pub fn distinct_text_map_last(
        &self,
        node: u32,
        collection: &[&str],
        key: &[&str],
        mut map: impl FnMut(&Self) -> Result<Value, RuntimeError>,
    ) -> Result<Value, RuntimeError> {
        let mut seen = functions::DistinctTextKeys::default();
        let mut selected = Vec::new();
        for item in self.aggregate_items(collection) {
            let mut value = item.current_instance();
            for segment in key {
                value = value.and_then(|value| value.field(segment));
            }
            let key = match value {
                None => Value::Null,
                Some(crate::Instance::Scalar(value)) => value.clone(),
                Some(_) => {
                    return Err(RuntimeError::DistinctTextMapKind {
                        node,
                        phase: "key",
                        found: "structured value",
                    });
                }
            };
            if seen.admit(&key).map_err(|error| match error {
                functions::DistinctTextKeyError::Kind(found) => RuntimeError::DistinctTextMapKind {
                    node,
                    phase: "key",
                    found,
                },
                functions::DistinctTextKeyError::Limit { resource, maximum } => {
                    RuntimeError::DistinctTextMapLimit {
                        node,
                        resource,
                        maximum,
                    }
                }
                functions::DistinctTextKeyError::InvalidLimits => unreachable!("default limits"),
            })? {
                selected.push(item);
            }
        }
        let mut last = Value::Null;
        for item in selected {
            match map(&item)? {
                Value::Null => {}
                value @ Value::String(_) => last = value,
                value => {
                    return Err(RuntimeError::DistinctTextMapKind {
                        node,
                        phase: "mapped value",
                        found: value.type_name(),
                    });
                }
            }
        }
        Ok(last)
    }
}
