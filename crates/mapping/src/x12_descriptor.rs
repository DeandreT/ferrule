//! Embedded descriptor shared by generated X12 boundaries and their runtime.
//!
//! The generator serializes this value into generated programs and the Rust
//! and C# runtimes parse the same JSON, so field order and omitted members
//! are part of the generated output.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{EdiImpliedDecimal, EdiLexicalFormat, EdiValueConstraint, X12Autocomplete};

/// One selected raw X12 side, as embedded in generated code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct X12Descriptor {
    pub version: X12DescriptorVersion,
    /// Embedded schema envelope produced by `codegen-schema`.
    pub schema: String,
    /// Always written; `null` (or a missing member) selects ISA discovery on
    /// input and default separators on output.
    #[serde(default)]
    pub separators: Option<X12DescriptorSeparators>,
    pub constraints: Vec<EdiValueConstraint>,
    pub lenient_segments: bool,
    pub implied_decimals: Vec<EdiImpliedDecimal>,
    pub lexical_formats: Vec<EdiLexicalFormat>,
    /// Required, but may be `null`.
    #[serde(deserialize_with = "required_nullable")]
    pub autocomplete: Option<X12Autocomplete>,
    /// Omitted for a single transaction; an explicit `null` is refused.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub envelope_profile: Option<X12DescriptorEnvelope>,
}

/// Version proved by the fixed ISA12/GS08 pair of the selected schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum X12DescriptorVersion {
    #[serde(rename = "004010")]
    V004010,
    #[serde(rename = "005010")]
    V005010,
    #[serde(rename = "006040")]
    V006040,
}

impl X12DescriptorVersion {
    /// Parse the six-digit GS08 version label.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "004010" => Some(Self::V004010),
            "005010" => Some(Self::V005010),
            "006040" => Some(Self::V006040),
            _ => None,
        }
    }

    /// The five-digit ISA12 interchange control version.
    pub const fn interchange(self) -> &'static str {
        match self {
            Self::V004010 => "00401",
            Self::V005010 => "00501",
            Self::V006040 => "00604",
        }
    }
}

/// Non-default envelope ownership grammars. A single transaction is the
/// absence of this member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum X12DescriptorEnvelope {
    #[serde(rename = "grouped_transactions")]
    GroupedTransactions,
    #[serde(rename = "mapforce_interchanges")]
    MapForceInterchanges,
}

/// Selected separators. The release character is outside the generated
/// profile and is never written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct X12DescriptorSeparators {
    pub element: char,
    pub component: char,
    pub segment: char,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition: Option<char>,
}

/// A required member whose value may be `null`. Using `deserialize_with`
/// keeps serde from treating a missing `Option` member as `None`.
fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

/// An optional member that, when present, must hold a value rather than `null`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> X12Descriptor {
        X12Descriptor {
            version: X12DescriptorVersion::V004010,
            schema: "s".into(),
            separators: None,
            constraints: Vec::new(),
            lenient_segments: false,
            implied_decimals: Vec::new(),
            lexical_formats: Vec::new(),
            autocomplete: None,
            envelope_profile: None,
        }
    }

    #[test]
    fn serialized_members_keep_their_order_and_omissions() {
        let mut descriptor = minimal();
        assert_eq!(
            serde_json::to_string(&descriptor).unwrap(),
            r#"{"version":"004010","schema":"s","separators":null,"constraints":[],"lenient_segments":false,"implied_decimals":[],"lexical_formats":[],"autocomplete":null}"#
        );
        descriptor.version = X12DescriptorVersion::V005010;
        descriptor.separators = Some(X12DescriptorSeparators {
            element: '*',
            component: ':',
            segment: '~',
            repetition: Some('^'),
        });
        descriptor.envelope_profile = Some(X12DescriptorEnvelope::MapForceInterchanges);
        let text = serde_json::to_string(&descriptor).unwrap();
        assert_eq!(
            text,
            r#"{"version":"005010","schema":"s","separators":{"element":"*","component":":","segment":"~","repetition":"^"},"constraints":[],"lenient_segments":false,"implied_decimals":[],"lexical_formats":[],"autocomplete":null,"envelope_profile":"mapforce_interchanges"}"#
        );
        assert_eq!(
            serde_json::from_str::<X12Descriptor>(&text).unwrap(),
            descriptor
        );
    }

    #[test]
    fn unknown_missing_and_null_members_are_refused() {
        let base = serde_json::to_value(minimal()).unwrap();
        let refused = |edit: &dyn Fn(&mut serde_json::Value)| {
            let mut value = base.clone();
            edit(&mut value);
            serde_json::from_value::<X12Descriptor>(value).is_err()
        };
        assert!(refused(&|value| value["extra"] = true.into()));
        assert!(refused(&|value| {
            value.as_object_mut().unwrap().remove("autocomplete");
        }));
        assert!(refused(
            &|value| value["envelope_profile"] = serde_json::Value::Null
        ));
        assert!(refused(&|value| {
            value["envelope_profile"] = "single_transaction".into();
        }));
        assert!(refused(&|value| value["version"] = "003040".into()));
        assert!(!refused(&|value| {
            value.as_object_mut().unwrap().remove("separators");
        }));
    }
}
