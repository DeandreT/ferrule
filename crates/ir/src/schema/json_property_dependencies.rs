use std::collections::BTreeMap;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_JSON_PROPERTY_DEPENDENCY_TRIGGERS: usize = 256;
pub const MAX_JSON_PROPERTY_DEPENDENCY_EDGES: usize = 4096;
pub const MAX_JSON_PROPERTY_DEPENDENCY_NAME_BYTES: usize = 256 * 1024;

/// Canonical JSON Schema property-presence implications.
///
/// Each key is one present trigger property and its sorted values are the
/// additional property names that must then be present. An absent
/// [`SchemaNode::json_property_dependencies`](crate::SchemaNode::json_property_dependencies)
/// represents no implications.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct JsonPropertyDependencies(BTreeMap<String, Vec<String>>);

impl JsonPropertyDependencies {
    pub fn new(
        mut rules: BTreeMap<String, Vec<String>>,
    ) -> Result<Self, JsonPropertyDependenciesError> {
        if rules.is_empty() {
            return Err(JsonPropertyDependenciesError::Empty);
        }
        for requirements in rules.values_mut() {
            requirements.sort();
        }
        Self::from_canonical(rules)
    }

    pub fn rules(&self) -> &BTreeMap<String, Vec<String>> {
        &self.0
    }

    pub fn requirements(&self, trigger: &str) -> Option<&[String]> {
        self.0.get(trigger).map(Vec::as_slice)
    }

    pub fn union(&self, other: &Self) -> Result<Self, JsonPropertyDependenciesError> {
        let mut rules = self.0.clone();
        for (trigger, requirements) in &other.0 {
            let target = rules.entry(trigger.clone()).or_default();
            for requirement in requirements {
                match target.binary_search(requirement) {
                    Ok(_) => {}
                    Err(position) => target.insert(position, requirement.clone()),
                }
            }
        }
        Self::from_canonical(rules)
    }

    fn from_canonical(
        rules: BTreeMap<String, Vec<String>>,
    ) -> Result<Self, JsonPropertyDependenciesError> {
        validate_rules(&rules)?;
        Ok(Self(rules))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonPropertyDependenciesError {
    Empty,
    EmptyRequirements,
    SelfDependency,
    DuplicateTrigger,
    DuplicateRequirement,
    TooManyTriggers,
    TooManyEdges,
    TooManyNameBytes,
}

impl core::fmt::Display for JsonPropertyDependenciesError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "JSON property dependencies must not be empty"),
            Self::EmptyRequirements => write!(
                formatter,
                "a JSON property dependency rule must require at least one property"
            ),
            Self::SelfDependency => {
                write!(
                    formatter,
                    "a JSON property dependency must not require itself"
                )
            }
            Self::DuplicateTrigger => {
                write!(
                    formatter,
                    "JSON property dependencies contain a duplicate trigger"
                )
            }
            Self::DuplicateRequirement => write!(
                formatter,
                "a JSON property dependency rule contains a duplicate target"
            ),
            Self::TooManyTriggers => write!(
                formatter,
                "JSON property dependencies exceed the {MAX_JSON_PROPERTY_DEPENDENCY_TRIGGERS}-trigger limit"
            ),
            Self::TooManyEdges => write!(
                formatter,
                "JSON property dependencies exceed the {MAX_JSON_PROPERTY_DEPENDENCY_EDGES}-edge limit"
            ),
            Self::TooManyNameBytes => write!(
                formatter,
                "JSON property dependency names exceed the {MAX_JSON_PROPERTY_DEPENDENCY_NAME_BYTES}-byte total limit"
            ),
        }
    }
}

impl std::error::Error for JsonPropertyDependenciesError {}

impl<'de> Deserialize<'de> for JsonPropertyDependencies {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct DependenciesVisitor;

        impl<'de> Visitor<'de> for DependenciesVisitor {
            type Value = JsonPropertyDependencies;

            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("a non-empty object of property dependency arrays")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut rules = BTreeMap::new();
                while let Some((trigger, requirements)) = map.next_entry::<String, Vec<String>>()? {
                    if rules.insert(trigger, requirements).is_some() {
                        return Err(serde::de::Error::custom(
                            JsonPropertyDependenciesError::DuplicateTrigger,
                        ));
                    }
                }
                JsonPropertyDependencies::from_canonical(rules).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_map(DependenciesVisitor)
    }
}

fn validate_rules(
    rules: &BTreeMap<String, Vec<String>>,
) -> Result<(), JsonPropertyDependenciesError> {
    if rules.is_empty() {
        return Err(JsonPropertyDependenciesError::Empty);
    }
    if rules.len() > MAX_JSON_PROPERTY_DEPENDENCY_TRIGGERS {
        return Err(JsonPropertyDependenciesError::TooManyTriggers);
    }
    let mut edges = 0_usize;
    let mut name_bytes = 0_usize;
    for (trigger, requirements) in rules {
        name_bytes = name_bytes
            .checked_add(trigger.len())
            .ok_or(JsonPropertyDependenciesError::TooManyNameBytes)?;
        if requirements.is_empty() {
            return Err(JsonPropertyDependenciesError::EmptyRequirements);
        }
        let mut previous = None;
        for requirement in requirements {
            if requirement == trigger {
                return Err(JsonPropertyDependenciesError::SelfDependency);
            }
            if previous.is_some_and(|previous| previous >= requirement.as_str()) {
                return Err(JsonPropertyDependenciesError::DuplicateRequirement);
            }
            previous = Some(requirement.as_str());
            edges = edges
                .checked_add(1)
                .ok_or(JsonPropertyDependenciesError::TooManyEdges)?;
            name_bytes = name_bytes
                .checked_add(requirement.len())
                .ok_or(JsonPropertyDependenciesError::TooManyNameBytes)?;
        }
    }
    if edges > MAX_JSON_PROPERTY_DEPENDENCY_EDGES {
        return Err(JsonPropertyDependenciesError::TooManyEdges);
    }
    if name_bytes > MAX_JSON_PROPERTY_DEPENDENCY_NAME_BYTES {
        return Err(JsonPropertyDependenciesError::TooManyNameBytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_sorts_rules_and_union_deduplicates_edges() {
        let dependencies = JsonPropertyDependencies::new(BTreeMap::from([
            ("b".into(), vec!["d".into(), "c".into()]),
            ("a".into(), vec!["c".into()]),
        ]))
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            serde_json::to_string(&dependencies).unwrap_or_default(),
            r#"{"a":["c"],"b":["c","d"]}"#
        );

        let other = JsonPropertyDependencies::new(BTreeMap::from([("b".into(), vec!["c".into()])]))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            dependencies
                .union(&other)
                .unwrap_or_else(|error| panic!("{error}")),
            dependencies
        );
    }

    #[test]
    fn serialized_ir_rejects_noncanonical_or_invalid_rules() {
        for invalid in [
            r#"{}"#,
            r#"{"a":[]}"#,
            r#"{"a":["a"]}"#,
            r#"{"a":["b","b"]}"#,
            r#"{"a":["c","b"]}"#,
            r#"{"a":["b"],"a":["c"]}"#,
        ] {
            assert!(
                serde_json::from_str::<JsonPropertyDependencies>(invalid).is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn empty_property_names_are_valid_and_explicit_budgets_are_enforced() {
        let empty_names =
            JsonPropertyDependencies::new(BTreeMap::from([(String::new(), vec!["x".into()])]))
                .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            serde_json::to_string(&empty_names).unwrap_or_default(),
            r#"{"":["x"]}"#
        );
        assert!(
            JsonPropertyDependencies::new(BTreeMap::from([("x".into(), vec![String::new()])]))
                .is_ok()
        );

        let too_many_triggers = (0..=MAX_JSON_PROPERTY_DEPENDENCY_TRIGGERS)
            .map(|index| (format!("trigger-{index}"), vec!["target".into()]))
            .collect();
        assert_eq!(
            JsonPropertyDependencies::new(too_many_triggers),
            Err(JsonPropertyDependenciesError::TooManyTriggers)
        );
        let too_many_edges = (0..=MAX_JSON_PROPERTY_DEPENDENCY_EDGES)
            .map(|index| format!("target-{index:04}"))
            .collect();
        assert_eq!(
            JsonPropertyDependencies::new(BTreeMap::from([("trigger".into(), too_many_edges)])),
            Err(JsonPropertyDependenciesError::TooManyEdges)
        );
        assert_eq!(
            JsonPropertyDependencies::new(BTreeMap::from([(
                "trigger".into(),
                vec!["x".repeat(MAX_JSON_PROPERTY_DEPENDENCY_NAME_BYTES)]
            )])),
            Err(JsonPropertyDependenciesError::TooManyNameBytes)
        );
    }
}
