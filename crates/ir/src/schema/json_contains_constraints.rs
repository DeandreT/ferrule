use serde::{Deserialize, Serialize};

use crate::{ItemCountRange, JsonSchemaPredicate, SchemaNode};

pub const MAX_JSON_CONTAINS_CONSTRAINTS: usize = 32;

/// One independently counted JSON Schema `contains` assertion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonContainsConstraint {
    predicate: JsonSchemaPredicate,
    range: ItemCountRange,
}

impl JsonContainsConstraint {
    pub fn new(predicate: JsonSchemaPredicate, range: ItemCountRange) -> Self {
        Self { predicate, range }
    }

    pub fn predicate(&self) -> &JsonSchemaPredicate {
        &self.predicate
    }

    pub fn range(&self) -> ItemCountRange {
        self.range
    }

    fn is_tautological(&self) -> bool {
        self.predicate.is_never() && self.range.contains_count(0)
    }
}

/// A bounded conjunction of independent JSON Schema `contains` assertions.
///
/// Declaration order is retained for deterministic diagnostics and export.
/// Duplicate terms are removed by [`Self::new`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct JsonContainsConstraints(Vec<JsonContainsConstraint>);

impl JsonContainsConstraints {
    pub fn new(constraints: impl IntoIterator<Item = JsonContainsConstraint>) -> Option<Self> {
        let mut canonical = Vec::new();
        for constraint in constraints {
            if constraint.is_tautological() || canonical.contains(&constraint) {
                continue;
            }
            if canonical.len() == MAX_JSON_CONTAINS_CONSTRAINTS {
                return None;
            }
            canonical.push(constraint);
        }
        (!canonical.is_empty()).then_some(Self(canonical))
    }

    pub fn as_slice(&self) -> &[JsonContainsConstraint] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<JsonContainsConstraint> {
        self.0
    }

    pub fn is_canonical(&self) -> bool {
        !self.0.is_empty()
            && self.0.len() <= MAX_JSON_CONTAINS_CONSTRAINTS
            && self.0.iter().enumerate().all(|(index, constraint)| {
                !constraint.is_tautological()
                    && !self.0[..index].contains(constraint)
                    && constraint
                        .predicate()
                        .as_schema()
                        .is_none_or(SchemaNode::metadata_is_valid)
            })
    }
}

impl<'de> Deserialize<'de> for JsonContainsConstraints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let constraints = Vec::<JsonContainsConstraint>::deserialize(deserializer)?;
        let canonical = Self::new(constraints.clone()).ok_or_else(|| {
            serde::de::Error::custom(
                "contains constraints must be nonempty, bounded, and non-tautological",
            )
        })?;
        (canonical.0 == constraints)
            .then_some(canonical)
            .ok_or_else(|| {
                serde::de::Error::custom(
                    "contains constraints must be canonical and contain no duplicates",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{JsonContainsConstraint, JsonContainsConstraints, MAX_JSON_CONTAINS_CONSTRAINTS};
    use crate::{ItemCountRange, JsonContainsPredicate, ScalarType, SchemaNode};

    #[test]
    fn construction_deduplicates_terms_and_removes_false_tautologies() {
        let Some(positive) = ItemCountRange::new(1, None) else {
            panic!("positive range is valid");
        };
        let Some(at_most_two) = ItemCountRange::new(0, Some(2)) else {
            panic!("bounded range is valid");
        };
        let constraint = JsonContainsConstraint::new(
            JsonContainsPredicate::schema(SchemaNode::scalar("item", ScalarType::Int)),
            positive,
        );
        let Some(constraints) = JsonContainsConstraints::new([
            JsonContainsConstraint::new(JsonContainsPredicate::never(), at_most_two),
            constraint.clone(),
            constraint,
        ]) else {
            panic!("one effective constraint remains");
        };
        assert_eq!(constraints.as_slice().len(), 1);
    }

    #[test]
    fn serde_rejects_noncanonical_and_unbounded_lists() {
        let duplicate = r#"[
          {"predicate":{"kind":"never"},"range":{"minimum":1}},
          {"predicate":{"kind":"never"},"range":{"minimum":1}}
        ]"#;
        assert!(serde_json::from_str::<JsonContainsConstraints>(duplicate).is_err());

        let term = serde_json::json!({
            "predicate":{"kind":"never"},
            "range":{"minimum":1}
        });
        let excessive = serde_json::Value::Array(
            core::iter::repeat_n(term, MAX_JSON_CONTAINS_CONSTRAINTS + 1).collect(),
        );
        assert!(serde_json::from_value::<JsonContainsConstraints>(excessive).is_err());
    }
}
