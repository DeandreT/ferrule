use serde::{Deserialize, Serialize};

use super::{JsonFormatAnnotations, JsonPatternConstraints, StringLengthRange};

pub const MAX_JSON_PROPERTY_NAMES: usize = 4096;
pub const MAX_JSON_PROPERTY_NAME_BYTES: usize = 256 * 1024;
pub const MAX_JSON_PROPERTY_NAME_TOTAL_BYTES: usize = 1024 * 1024;

/// One canonical finite domain of JSON object member names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct JsonPropertyNameSet(Vec<String>);

impl JsonPropertyNameSet {
    pub fn new(names: impl IntoIterator<Item = String>) -> Result<Self, JsonPropertyNameSetError> {
        let mut names = names.into_iter().collect::<Vec<_>>();
        names.sort();
        names.dedup();
        Self::from_canonical(names)
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub fn contains(&self, name: &str) -> bool {
        self.0
            .binary_search_by(|candidate| candidate.as_str().cmp(name))
            .is_ok()
    }

    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let mut names = Vec::new();
        let mut left = self.0.iter();
        let mut right = other.0.iter();
        let mut left_name = left.next();
        let mut right_name = right.next();
        while let (Some(left_value), Some(right_value)) = (left_name, right_name) {
            match left_value.cmp(right_value) {
                core::cmp::Ordering::Less => left_name = left.next(),
                core::cmp::Ordering::Greater => right_name = right.next(),
                core::cmp::Ordering::Equal => {
                    names.push(left_value.clone());
                    left_name = left.next();
                    right_name = right.next();
                }
            }
        }
        (!names.is_empty()).then_some(Self(names))
    }

    pub fn union(&self, other: &Self) -> Result<Self, JsonPropertyNameSetError> {
        Self::new(self.0.iter().chain(&other.0).cloned())
    }

    fn from_canonical(names: Vec<String>) -> Result<Self, JsonPropertyNameSetError> {
        validate_names(&names)?;
        Ok(Self(names))
    }
}

impl<'de> Deserialize<'de> for JsonPropertyNameSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let names = Vec::<String>::deserialize(deserializer)?;
        Self::from_canonical(names).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonPropertyNameSetError {
    Empty,
    DuplicateOrUnsorted,
    TooMany,
    NameTooLong,
    TooManyBytes,
}

impl core::fmt::Display for JsonPropertyNameSetError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "JSON property-name set must not be empty"),
            Self::DuplicateOrUnsorted => write!(
                formatter,
                "serialized JSON property-name sets must be strictly sorted and unique"
            ),
            Self::TooMany => write!(
                formatter,
                "JSON property-name set exceeds the {MAX_JSON_PROPERTY_NAMES}-name limit"
            ),
            Self::NameTooLong => write!(
                formatter,
                "a JSON property name exceeds the {MAX_JSON_PROPERTY_NAME_BYTES}-byte limit"
            ),
            Self::TooManyBytes => write!(
                formatter,
                "JSON property names exceed the {MAX_JSON_PROPERTY_NAME_TOTAL_BYTES}-byte total limit"
            ),
        }
    }
}

impl std::error::Error for JsonPropertyNameSetError {}

/// Exact portable assertions applied to every name in one JSON object.
///
/// Absence of this metadata means the unrestricted `propertyNames: true`
/// schema. `Never` rejects every member name, while `Schema` is a conjunction
/// of its present dimensions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JsonPropertyNameConstraints {
    Never,
    Schema {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        allowed: Option<JsonPropertyNameSet>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        excluded: Option<JsonPropertyNameSet>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        length: Option<StringLengthRange>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        patterns: Option<JsonPatternConstraints>,
        #[serde(default, skip_serializing_if = "JsonFormatAnnotations::is_empty")]
        formats: JsonFormatAnnotations,
    },
}

impl JsonPropertyNameConstraints {
    pub fn schema(
        allowed: Option<JsonPropertyNameSet>,
        length: Option<StringLengthRange>,
        patterns: Option<JsonPatternConstraints>,
        formats: JsonFormatAnnotations,
    ) -> Option<Self> {
        Self::schema_excluding(allowed, None, length, patterns, formats)
    }

    pub fn schema_excluding(
        allowed: Option<JsonPropertyNameSet>,
        excluded: Option<JsonPropertyNameSet>,
        length: Option<StringLengthRange>,
        patterns: Option<JsonPatternConstraints>,
        formats: JsonFormatAnnotations,
    ) -> Option<Self> {
        let patterns = patterns.filter(|patterns| !patterns.is_tautology());
        if allowed.is_none()
            && excluded.is_none()
            && length.is_none()
            && patterns.is_none()
            && formats.is_empty()
        {
            return None;
        }
        let had_finite_domain = allowed.is_some();
        let allowed = allowed.and_then(|allowed| {
            let names = allowed
                .as_slice()
                .iter()
                .filter(|name| {
                    length.is_none_or(|range| range.contains_str(name))
                        && patterns
                            .as_ref()
                            .is_none_or(|constraints| constraints.matches(name))
                        && excluded
                            .as_ref()
                            .is_none_or(|excluded| !excluded.contains(name))
                })
                .cloned()
                .collect::<Vec<_>>();
            (!names.is_empty()).then_some(JsonPropertyNameSet(names))
        });
        if had_finite_domain && allowed.is_none() {
            return Some(Self::Never);
        }
        let excluded = allowed.is_none().then_some(excluded).flatten();
        Some(Self::Schema {
            allowed,
            excluded,
            length,
            patterns,
            formats,
        })
    }

    pub fn never() -> Self {
        Self::Never
    }

    pub fn allowed(&self) -> Option<&JsonPropertyNameSet> {
        match self {
            Self::Never => None,
            Self::Schema { allowed, .. } => allowed.as_ref(),
        }
    }

    pub fn excluded(&self) -> Option<&JsonPropertyNameSet> {
        match self {
            Self::Never => None,
            Self::Schema { excluded, .. } => excluded.as_ref(),
        }
    }

    pub fn length(&self) -> Option<StringLengthRange> {
        match self {
            Self::Never => None,
            Self::Schema { length, .. } => *length,
        }
    }

    pub fn patterns(&self) -> Option<&JsonPatternConstraints> {
        match self {
            Self::Never => None,
            Self::Schema { patterns, .. } => patterns.as_ref(),
        }
    }

    pub fn formats(&self) -> Option<&JsonFormatAnnotations> {
        match self {
            Self::Never => None,
            Self::Schema { formats, .. } => Some(formats),
        }
    }

    pub fn accepts(&self, name: &str) -> bool {
        match self {
            Self::Never => false,
            Self::Schema {
                allowed,
                excluded,
                length,
                patterns,
                ..
            } => {
                allowed
                    .as_ref()
                    .is_none_or(|allowed| allowed.contains(name))
                    && excluded
                        .as_ref()
                        .is_none_or(|excluded| !excluded.contains(name))
                    && length.is_none_or(|length| length.contains_str(name))
                    && patterns
                        .as_ref()
                        .is_none_or(|patterns| patterns.matches(name))
            }
        }
    }

    pub fn is_canonical(&self) -> bool {
        self.is_structurally_canonical()
            && match self {
                Self::Never => true,
                Self::Schema {
                    allowed,
                    excluded,
                    patterns,
                    ..
                } => allowed.as_ref().is_none_or(|allowed| {
                    excluded.is_none()
                        && allowed.as_slice().iter().all(|name| {
                            patterns
                                .as_ref()
                                .is_none_or(|patterns| patterns.matches(name))
                        })
                }),
            }
    }

    pub(crate) fn is_structurally_canonical(&self) -> bool {
        match self {
            Self::Never => true,
            Self::Schema {
                allowed,
                excluded,
                length,
                patterns,
                formats,
            } => {
                if allowed.is_none()
                    && excluded.is_none()
                    && length.is_none()
                    && patterns.is_none()
                    && formats.is_empty()
                {
                    return false;
                }
                if allowed.is_some() && excluded.is_some() {
                    return false;
                }
                if patterns
                    .as_ref()
                    .is_some_and(JsonPatternConstraints::is_tautology)
                {
                    return false;
                }
                allowed.as_ref().is_none_or(|allowed| {
                    allowed
                        .as_slice()
                        .iter()
                        .all(|name| length.is_none_or(|length| length.contains_str(name)))
                })
            }
        }
    }

    pub(crate) fn accepts_without_patterns(&self, name: &str) -> bool {
        match self {
            Self::Never => false,
            Self::Schema {
                allowed,
                excluded,
                length,
                ..
            } => {
                allowed
                    .as_ref()
                    .is_none_or(|allowed| allowed.contains(name))
                    && excluded
                        .as_ref()
                        .is_none_or(|excluded| !excluded.contains(name))
                    && length.is_none_or(|length| length.contains_str(name))
            }
        }
    }

    pub fn finite_capacity(&self) -> Option<usize> {
        match self {
            Self::Never => Some(0),
            Self::Schema {
                allowed: Some(allowed),
                ..
            } => Some(allowed.as_slice().len()),
            Self::Schema { allowed: None, .. } => None,
        }
    }
}

impl<'de> Deserialize<'de> for JsonPropertyNameConstraints {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Repr {
            Never,
            Schema {
                #[serde(default)]
                allowed: Option<JsonPropertyNameSet>,
                #[serde(default)]
                excluded: Option<JsonPropertyNameSet>,
                #[serde(default)]
                length: Option<StringLengthRange>,
                #[serde(default)]
                patterns: Option<JsonPatternConstraints>,
                #[serde(default)]
                formats: JsonFormatAnnotations,
            },
        }

        match Repr::deserialize(deserializer)? {
            Repr::Never => Ok(Self::Never),
            Repr::Schema {
                allowed,
                excluded,
                length,
                patterns,
                formats,
            } => {
                let constraints = Self::Schema {
                    allowed,
                    excluded,
                    length,
                    patterns,
                    formats,
                };
                let canonical = if crate::schema_deserialization_is_active() {
                    constraints.is_structurally_canonical()
                } else {
                    constraints.is_canonical()
                };
                if !canonical {
                    return Err(serde::de::Error::custom(
                        "JSON property-name constraints are tautological or contain a finite name outside their other assertions",
                    ));
                }
                Ok(constraints)
            }
        }
    }
}

fn validate_names(names: &[String]) -> Result<(), JsonPropertyNameSetError> {
    if names.is_empty() {
        return Err(JsonPropertyNameSetError::Empty);
    }
    if names.len() > MAX_JSON_PROPERTY_NAMES {
        return Err(JsonPropertyNameSetError::TooMany);
    }
    let mut total_bytes = 0_usize;
    let mut previous = None;
    for name in names {
        if previous.is_some_and(|previous| previous >= name.as_str()) {
            return Err(JsonPropertyNameSetError::DuplicateOrUnsorted);
        }
        previous = Some(name.as_str());
        if name.len() > MAX_JSON_PROPERTY_NAME_BYTES {
            return Err(JsonPropertyNameSetError::NameTooLong);
        }
        total_bytes = total_bytes
            .checked_add(name.len())
            .ok_or(JsonPropertyNameSetError::TooManyBytes)?;
    }
    if total_bytes > MAX_JSON_PROPERTY_NAME_TOTAL_BYTES {
        return Err(JsonPropertyNameSetError::TooManyBytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_names_are_sorted_and_serialized_canonically() {
        let names = JsonPropertyNameSet::new(["z".to_string(), String::new(), "a".to_string()])
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(names.as_slice(), ["", "a", "z"]);
        assert_eq!(
            serde_json::to_string(&names).unwrap_or_default(),
            r#"["","a","z"]"#
        );
        assert!(serde_json::from_str::<JsonPropertyNameSet>(r#"["z","a"]"#).is_err());
    }

    #[test]
    fn tagged_constraints_reject_the_tautological_schema_variant() {
        assert!(
            serde_json::from_str::<JsonPropertyNameConstraints>(r#"{"kind":"schema"}"#).is_err()
        );
        assert!(
            serde_json::from_str::<JsonPropertyNameConstraints>(
                r#"{"kind":"schema","allowed":["bad"],"patterns":{"any_of":[["^good$"]]}}"#,
            )
            .is_err()
        );
        assert_eq!(
            serde_json::to_string(&JsonPropertyNameConstraints::never()).unwrap_or_default(),
            r#"{"kind":"never"}"#
        );
        let excluded = JsonPropertyNameSet::new(["blocked".to_string()])
            .unwrap_or_else(|error| panic!("{error}"));
        let constraints = JsonPropertyNameConstraints::schema_excluding(
            None,
            Some(excluded),
            None,
            None,
            JsonFormatAnnotations::default(),
        )
        .unwrap_or_else(|| panic!("a finite exclusion is not tautological"));
        assert!(!constraints.accepts("blocked"));
        assert!(constraints.accepts("allowed"));
        assert_eq!(
            serde_json::to_string(&constraints).unwrap_or_default(),
            r#"{"kind":"schema","excluded":["blocked"]}"#
        );
        assert_eq!(
            serde_json::from_str::<JsonPropertyNameConstraints>(
                r#"{"kind":"schema","excluded":["blocked"]}"#
            )
            .unwrap_or_else(|error| panic!("{error}")),
            constraints
        );
        assert!(
            serde_json::from_str::<JsonPropertyNameConstraints>(
                r#"{"kind":"schema","allowed":["a"],"excluded":["b"]}"#
            )
            .is_err()
        );
    }
}
