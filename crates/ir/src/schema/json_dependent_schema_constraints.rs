use serde::{Deserialize, Serialize};

use crate::JsonSchemaPredicate;

use super::MAX_JSON_PROPERTY_DEPENDENCY_NAME_BYTES;

/// Maximum nontrivial `dependentSchemas` assertions retained on one object.
pub const MAX_JSON_DEPENDENT_SCHEMA_CONSTRAINTS: usize = 32;

/// One conditional whole-object JSON Schema assertion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonDependentSchemaConstraint {
    trigger: String,
    predicate: JsonSchemaPredicate,
}

impl JsonDependentSchemaConstraint {
    pub fn new(trigger: impl Into<String>, predicate: JsonSchemaPredicate) -> Self {
        Self {
            trigger: trigger.into(),
            predicate,
        }
    }

    pub fn trigger(&self) -> &str {
        &self.trigger
    }

    pub fn predicate(&self) -> &JsonSchemaPredicate {
        &self.predicate
    }

    fn is_tautological(&self) -> bool {
        self.predicate
            .as_schema()
            .is_some_and(|schema| schema.json_any && !schema.repeating)
    }
}

/// A bounded ordered conjunction of object-triggered schema predicates.
///
/// Repeated triggers are intentional: separate `allOf` branches may impose
/// multiple independent schemas when the same property is present.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct JsonDependentSchemaConstraints(Vec<JsonDependentSchemaConstraint>);

impl JsonDependentSchemaConstraints {
    pub fn new(
        constraints: impl IntoIterator<Item = JsonDependentSchemaConstraint>,
    ) -> Option<Self> {
        let mut canonical = Vec::new();
        let mut trigger_bytes = 0_usize;
        for constraint in constraints {
            if constraint.is_tautological() || canonical.contains(&constraint) {
                continue;
            }
            if canonical.len() == MAX_JSON_DEPENDENT_SCHEMA_CONSTRAINTS {
                return None;
            }
            trigger_bytes = trigger_bytes.checked_add(constraint.trigger.len())?;
            if trigger_bytes > MAX_JSON_PROPERTY_DEPENDENCY_NAME_BYTES {
                return None;
            }
            canonical.push(constraint);
        }
        (!canonical.is_empty()).then_some(Self(canonical))
    }

    pub fn as_slice(&self) -> &[JsonDependentSchemaConstraint] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<JsonDependentSchemaConstraint> {
        self.0
    }

    /// Returns whether both ordered collections represent the same conjunction.
    ///
    /// Constraint declaration order is retained for deterministic diagnostics
    /// and export, but does not affect JSON Schema conjunction semantics.
    pub fn semantically_equals(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().all(|constraint| other.0.contains(constraint))
    }

    /// Checks only the bounded collection invariants.
    ///
    /// The owning [`crate::SchemaNode`] validates each private predicate schema
    /// tree once; doing that here as well makes nested predicates exponential.
    pub fn is_canonical(&self) -> bool {
        if self.0.is_empty() || self.0.len() > MAX_JSON_DEPENDENT_SCHEMA_CONSTRAINTS {
            return false;
        }
        let mut trigger_bytes = 0_usize;
        self.0.iter().enumerate().all(|(index, constraint)| {
            trigger_bytes = match trigger_bytes.checked_add(constraint.trigger.len()) {
                Some(bytes) => bytes,
                None => return false,
            };
            trigger_bytes <= MAX_JSON_PROPERTY_DEPENDENCY_NAME_BYTES
                && !constraint.is_tautological()
                && !self.0[..index].contains(constraint)
        })
    }
}

impl<'de> Deserialize<'de> for JsonDependentSchemaConstraints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let constraints = Vec::<JsonDependentSchemaConstraint>::deserialize(deserializer)?;
        let Some(canonical) = Self::new(constraints.clone()) else {
            return Err(serde::de::Error::custom(
                "dependent schema constraints must be nonempty, bounded, and non-tautological",
            ));
        };
        if canonical.0 != constraints {
            return Err(serde::de::Error::custom(
                "dependent schema constraints must be canonical and contain no duplicates",
            ));
        }
        Ok(canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ScalarType, SchemaNode};

    #[test]
    fn construction_deduplicates_and_drops_unconstrained_predicates() {
        let predicate =
            JsonSchemaPredicate::schema(SchemaNode::scalar("value", ScalarType::String));
        let constraint = JsonDependentSchemaConstraint::new("trigger", predicate);
        let unconstrained = SchemaNode::scalar("any", ScalarType::String)
            .json_any()
            .unwrap_or_else(|| panic!("arbitrary JSON schema is valid"));
        let constraints = JsonDependentSchemaConstraints::new([
            constraint.clone(),
            constraint,
            JsonDependentSchemaConstraint::new(
                "ignored",
                JsonSchemaPredicate::schema(unconstrained),
            ),
        ])
        .unwrap_or_else(|| panic!("one nontrivial constraint remains"));
        assert_eq!(constraints.as_slice().len(), 1);
    }

    #[test]
    fn serde_rejects_duplicate_or_excessive_terms() {
        let duplicate = r#"[
          {"trigger":"a","predicate":{"kind":"never"}},
          {"trigger":"a","predicate":{"kind":"never"}}
        ]"#;
        assert!(serde_json::from_str::<JsonDependentSchemaConstraints>(duplicate).is_err());

        let terms = serde_json::Value::Array(
            (0..=MAX_JSON_DEPENDENT_SCHEMA_CONSTRAINTS)
                .map(|index| {
                    serde_json::json!({
                        "trigger": index.to_string(),
                        "predicate": {"kind":"never"}
                    })
                })
                .collect(),
        );
        assert!(serde_json::from_value::<JsonDependentSchemaConstraints>(terms).is_err());
    }

    #[test]
    fn conjunction_equivalence_ignores_retained_declaration_order() {
        let left = JsonDependentSchemaConstraint::new("left", JsonSchemaPredicate::never());
        let right = JsonDependentSchemaConstraint::new("right", JsonSchemaPredicate::never());
        let constraints = JsonDependentSchemaConstraints::new([left.clone(), right.clone()])
            .unwrap_or_else(|| panic!("two distinct constraints are valid"));
        let reordered = JsonDependentSchemaConstraints::new([right, left])
            .unwrap_or_else(|| panic!("reordered distinct constraints are valid"));

        assert_ne!(constraints, reordered);
        assert!(constraints.semantically_equals(&reordered));
        assert_eq!(constraints.as_slice()[0].trigger(), "left");
    }

    #[test]
    fn deeply_nested_predicate_metadata_is_validated_once_per_tree() {
        fn wrap(predicate: SchemaNode, depth: usize) -> SchemaNode {
            (0..depth).fold(predicate, |predicate, index| {
                let constraints =
                    JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
                        "trigger",
                        JsonSchemaPredicate::schema(predicate),
                    )])
                    .unwrap_or_else(|| panic!("one nested predicate is valid"));
                let mut wrapper = SchemaNode::group(format!("level-{index}"), vec![]);
                wrapper.json_dependent_schemas = Some(constraints);
                wrapper
            })
        }

        let valid = wrap(SchemaNode::scalar("leaf", ScalarType::String), 48);
        assert!(valid.metadata_tree_is_valid());
        assert!(valid.json_dependent_schemas_tree_is_valid());

        let misplaced = JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
            "invalid",
            JsonSchemaPredicate::never(),
        )])
        .unwrap_or_else(|| panic!("one local constraint is canonical"));
        let mut invalid_leaf = SchemaNode::scalar("invalid-leaf", ScalarType::String);
        invalid_leaf.json_dependent_schemas = Some(misplaced);
        let invalid = wrap(invalid_leaf, 48);
        assert!(!invalid.metadata_tree_is_valid());
        assert!(!invalid.json_dependent_schemas_tree_is_valid());
    }

    #[test]
    fn predicate_tree_validation_remains_strict_for_serde_and_builders() {
        let misplaced = JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
            "invalid",
            JsonSchemaPredicate::never(),
        )])
        .unwrap_or_else(|| panic!("one local constraint is canonical"));
        let mut invalid_predicate = SchemaNode::scalar("invalid", ScalarType::String);
        invalid_predicate.json_dependent_schemas = Some(misplaced);
        let constraints =
            JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
                "trigger",
                JsonSchemaPredicate::schema(invalid_predicate),
            )])
            .unwrap_or_else(|| panic!("the outer collection is locally canonical"));

        let encoded = serde_json::to_string(&constraints).unwrap_or_else(|error| panic!("{error}"));
        assert!(
            serde_json::from_str::<JsonDependentSchemaConstraints>(&encoded).is_err(),
            "nested invalid schema metadata must fail deserialization"
        );
        assert!(
            SchemaNode::group("root", vec![])
                .with_json_dependent_schemas(constraints)
                .is_none(),
            "programmatic attachment must reject nested invalid schema metadata"
        );
    }
}
