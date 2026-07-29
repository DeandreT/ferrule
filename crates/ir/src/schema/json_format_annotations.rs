use serde::{Deserialize, Serialize};

pub const MAX_JSON_FORMAT_ANNOTATIONS: usize = 64;
pub const MAX_JSON_FORMAT_ANNOTATION_BYTES: usize = 1024;
pub const MAX_JSON_FORMAT_ANNOTATION_TOTAL_BYTES: usize = 16 * 1024;

/// A bounded, ordered set of JSON Schema `format` annotation strings.
///
/// Unknown and empty annotation names remain meaningful annotations and are
/// retained exactly. Duplicate names are omitted when annotations are
/// accumulated programmatically and rejected in serialized IR so the wire
/// representation stays canonical.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct JsonFormatAnnotations(Vec<String>);

impl JsonFormatAnnotations {
    pub fn new(
        annotations: impl IntoIterator<Item = String>,
    ) -> Result<Self, JsonFormatAnnotationsError> {
        let mut result = Self::default();
        result.extend(annotations)?;
        Ok(result)
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn into_vec(self) -> Vec<String> {
        self.0
    }

    pub fn extend(
        &mut self,
        annotations: impl IntoIterator<Item = String>,
    ) -> Result<(), JsonFormatAnnotationsError> {
        for annotation in annotations {
            if self.0.contains(&annotation) {
                continue;
            }
            validate_annotation(&annotation)?;
            if self.0.len() == MAX_JSON_FORMAT_ANNOTATIONS {
                return Err(JsonFormatAnnotationsError::TooMany);
            }
            let total_bytes = self
                .0
                .iter()
                .map(String::len)
                .sum::<usize>()
                .checked_add(annotation.len())
                .ok_or(JsonFormatAnnotationsError::TooManyBytes)?;
            if total_bytes > MAX_JSON_FORMAT_ANNOTATION_TOTAL_BYTES {
                return Err(JsonFormatAnnotationsError::TooManyBytes);
            }
            self.0.push(annotation);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonFormatAnnotationsError {
    TooMany,
    AnnotationTooLong,
    TooManyBytes,
    Duplicate,
}

impl core::fmt::Display for JsonFormatAnnotationsError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooMany => write!(
                formatter,
                "JSON format annotations exceed the {MAX_JSON_FORMAT_ANNOTATIONS}-annotation limit"
            ),
            Self::AnnotationTooLong => write!(
                formatter,
                "a JSON format annotation exceeds the {MAX_JSON_FORMAT_ANNOTATION_BYTES}-byte limit"
            ),
            Self::TooManyBytes => write!(
                formatter,
                "JSON format annotations exceed the {MAX_JSON_FORMAT_ANNOTATION_TOTAL_BYTES}-byte total limit"
            ),
            Self::Duplicate => write!(formatter, "JSON format annotations contain a duplicate"),
        }
    }
}

impl std::error::Error for JsonFormatAnnotationsError {}

impl<'de> Deserialize<'de> for JsonFormatAnnotations {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let annotations = Vec::<String>::deserialize(deserializer)?;
        if annotations.len() > MAX_JSON_FORMAT_ANNOTATIONS {
            return Err(serde::de::Error::custom(
                JsonFormatAnnotationsError::TooMany,
            ));
        }
        for (index, annotation) in annotations.iter().enumerate() {
            validate_annotation(annotation).map_err(serde::de::Error::custom)?;
            if annotations[..index].contains(annotation) {
                return Err(serde::de::Error::custom(
                    JsonFormatAnnotationsError::Duplicate,
                ));
            }
        }
        let total_bytes = annotations
            .iter()
            .try_fold(0_usize, |total, annotation| {
                total.checked_add(annotation.len())
            })
            .ok_or_else(|| serde::de::Error::custom(JsonFormatAnnotationsError::TooManyBytes))?;
        if total_bytes > MAX_JSON_FORMAT_ANNOTATION_TOTAL_BYTES {
            return Err(serde::de::Error::custom(
                JsonFormatAnnotationsError::TooManyBytes,
            ));
        }
        Ok(Self(annotations))
    }
}

fn validate_annotation(annotation: &str) -> Result<(), JsonFormatAnnotationsError> {
    if annotation.len() > MAX_JSON_FORMAT_ANNOTATION_BYTES {
        return Err(JsonFormatAnnotationsError::AnnotationTooLong);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulation_is_ordered_deduplicated_and_preserves_empty_values() {
        let mut annotations =
            JsonFormatAnnotations::new([String::new(), "uuid".to_string()]).unwrap_or_default();
        assert!(
            annotations
                .extend(["uuid".to_string(), "custom".to_string()])
                .is_ok()
        );
        assert_eq!(annotations.as_slice(), ["", "uuid", "custom"]);
    }

    #[test]
    fn deserialization_rejects_noncanonical_or_unbounded_values() {
        assert!(serde_json::from_str::<JsonFormatAnnotations>(r#"["x","x"]"#).is_err());
        assert!(
            serde_json::from_str::<JsonFormatAnnotations>(&format!(
                "[{}]",
                serde_json::to_string(&"x".repeat(MAX_JSON_FORMAT_ANNOTATION_BYTES + 1))
                    .unwrap_or_default()
            ))
            .is_err()
        );
    }
}
