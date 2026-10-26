//! Retained native JSON5 metadata, not an executable JSON5 codec selection.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A native JSON5 boundary whose interchange semantics are not admitted.
/// Instance paths stay on their ordinary owning source or target; this value
/// preserves why those paths/settings cannot be interpreted as native JSON5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cause", rename_all = "snake_case", deny_unknown_fields)]
pub enum Json5InterchangeDependency {
    /// Retain the exact component attribute, including unrecognized spellings.
    ComponentSetting { value: String },
    /// A JSON5 filename with no component setting. The vendor does not use the
    /// filename to enable JSON5, while native file dispatch does.
    InstanceExtension {},
}

impl fmt::Display for Json5InterchangeDependency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "native JSON5 interchange is not executable: component settings, filename selection and numeric semantics differ from the native JSON5 codec",
        )
    }
}

impl std::error::Error for Json5InterchangeDependency {}

/// The role of a non-document JSON component retained by an MFD import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Json5StringComponentKind {
    Parser,
    Serializer,
}

/// Original settings of an internal string adapter. These live on the
/// primary source options beside other MFD-wide parser metadata, because the
/// adapter is lowered into graph expressions rather than a document endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Json5StringInterchangeDependency {
    pub component: String,
    pub kind: Json5StringComponentKind,
    pub dependency: Json5InterchangeDependency,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_instance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_instance: Option<String>,
}

impl crate::FormatOptions {
    /// A retained document or internal string component that forbids codec
    /// admission. The full original metadata remains on the options value.
    pub fn json5_interchange_blocker(&self) -> Option<&Json5InterchangeDependency> {
        self.json5_interchange_dependency.as_ref().or_else(|| {
            self.mfd_json5_string_dependencies
                .first()
                .map(|component| &component.dependency)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FormatOptions;

    #[test]
    fn exact_component_spellings_roundtrip_without_selecting_native_json5() {
        for value in ["1", "0", "true", "false", "", "invalid"] {
            let options = FormatOptions {
                json5_interchange_dependency: Some(Json5InterchangeDependency::ComponentSetting {
                    value: value.into(),
                }),
                ..FormatOptions::default()
            };
            let encoded = serde_json::to_value(&options).unwrap();
            assert_eq!(
                encoded["json5_interchange_dependency"]["cause"],
                "component_setting"
            );
            assert_eq!(encoded["json5_interchange_dependency"]["value"], value);
            let restored: FormatOptions = serde_json::from_value(encoded).unwrap();
            assert_eq!(restored, options);
            assert!(!restored.json5);
        }
    }

    #[test]
    fn absent_dependency_does_not_change_legacy_serialization() {
        let options = FormatOptions::default();
        let value = serde_json::to_value(&options).unwrap();
        assert!(
            !value
                .as_object()
                .unwrap()
                .contains_key("mfd_json5_string_dependencies")
        );
        assert!(
            !value
                .as_object()
                .unwrap()
                .contains_key("json5_interchange_dependency")
        );
        assert_eq!(
            serde_json::from_value::<FormatOptions>(value.clone()).unwrap(),
            options
        );
        assert_eq!(serde_json::to_value(options).unwrap(), value);
    }

    #[test]
    fn extension_cause_roundtrips_and_unknown_causes_refuse() {
        let dependency = Json5InterchangeDependency::InstanceExtension {};
        let encoded = serde_json::to_string(&dependency).unwrap();
        assert_eq!(
            serde_json::from_str::<Json5InterchangeDependency>(&encoded).unwrap(),
            dependency
        );
        for malformed in [
            r#"{"cause":"unknown"}"#,
            r#"{"cause":"component_setting"}"#,
            r#"{"cause":"instance_extension","value":"1"}"#,
        ] {
            assert!(serde_json::from_str::<Json5InterchangeDependency>(malformed).is_err());
        }
    }
}
