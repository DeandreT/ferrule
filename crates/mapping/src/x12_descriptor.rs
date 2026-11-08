//! Embedded descriptor shared by generated X12 boundaries and their runtime.
//!
//! The generator serializes this value into generated programs and the Rust
//! and C# runtimes parse the same JSON, so field order and omitted members
//! are part of the generated output.

use std::{fmt, marker::PhantomData};

use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::value::RawValue;

use crate::{EdiImpliedDecimal, EdiLexicalFormat, EdiValueConstraint, X12Autocomplete};

// Declare the wire members once. The staged view checks object membership up
// front, but leaves values as raw JSON so the runtime can decode/validate the
// schema before the remaining fields. RawValue also preserves nested duplicate
// members for the typed deserializers to refuse rather than silently overwrite.
macro_rules! descriptor {
    ($( $(#[$attr:meta])* pub $field:ident: $ty:ty => $decode:ident, )*) => {
        /// One selected raw X12 side, as embedded in generated code.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
        pub struct X12Descriptor {
            $( $(#[$attr])* pub $field: $ty, )*
        }

        impl<'de> Deserialize<'de> for X12Descriptor {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Members { $( $(#[$attr])* $field: $ty, )* }
                let members: Members = object_only(deserializer)?;
                Ok(Self { $( $field: members.$field, )* })
            }
        }

        /// Object-only descriptor view with typed, individually decoded members.
        ///
        /// The runtime checks its byte limit before parsing this view, then calls
        /// the accessors in its established validation order. Missing required
        /// values are deferred too; unknown and duplicate top-level keys are not.
        pub struct X12DescriptorView {
            $( $field: Option<Box<RawValue>>, )*
        }

        impl<'de> Deserialize<'de> for X12DescriptorView {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Members {
                    $( #[serde(default, deserialize_with = "present")]
                    $field: Option<Box<RawValue>>, )*
                }
                let members: Members = object_only(deserializer)?;
                Ok(Self { $( $field: members.$field, )* })
            }
        }

        impl X12DescriptorView {
            $( pub fn $field(&self) -> Result<$ty, serde_json::Error> {
                $decode(self.$field.as_deref(), stringify!($field))
            } )*
        }
    };
}

descriptor! {
    pub version: X12DescriptorVersion => required,
    /// Embedded schema envelope produced by `codegen-schema`.
    pub schema: String => required,
    /// Always written; `null` (or a missing member) selects ISA discovery on
    /// input and default separators on output.
    #[serde(default)]
    pub separators: Option<X12DescriptorSeparators> => defaulted,
    pub constraints: Vec<EdiValueConstraint> => required,
    pub lenient_segments: bool => required,
    pub implied_decimals: Vec<EdiImpliedDecimal> => required,
    pub lexical_formats: Vec<EdiLexicalFormat> => required,
    /// Required, but may be `null`.
    #[serde(deserialize_with = "required_nullable")]
    pub autocomplete: Option<X12Autocomplete> => required,
    /// Omitted for a single transaction; an explicit `null` is refused.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub envelope_profile: Option<X12DescriptorEnvelope> => optional_present,
}

fn required<T: de::DeserializeOwned>(
    value: Option<&RawValue>,
    name: &str,
) -> Result<T, serde_json::Error> {
    let value = value.ok_or_else(|| de::Error::custom(format!("missing {name}")))?;
    serde_json::from_str(value.get())
}

fn defaulted<T: de::DeserializeOwned + Default>(
    value: Option<&RawValue>,
    name: &str,
) -> Result<T, serde_json::Error> {
    match value {
        Some(_) => required(value, name),
        None => Ok(T::default()),
    }
}

fn optional_present<T: de::DeserializeOwned>(
    value: Option<&RawValue>,
    name: &str,
) -> Result<Option<T>, serde_json::Error> {
    value.map(|value| required(Some(value), name)).transpose()
}

// Serde's derived struct visitor also accepts positional sequences. This wire
// boundary has always required an object, including for nested separators.
fn object_only<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    struct Object<T>(PhantomData<T>);
    impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Object<T> {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object")
        }

        fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            T::deserialize(de::value::MapAccessDeserializer::new(map))
        }
    }
    deserializer.deserialize_map(Object(PhantomData))
}

/// Version proved by the fixed ISA12/GS08 pair of the selected schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub enum X12DescriptorVersion {
    #[serde(rename = "004010")]
    V004010,
    #[serde(rename = "005010")]
    V005010,
    #[serde(rename = "006040")]
    V006040,
}

impl TryFrom<String> for X12DescriptorVersion {
    type Error = &'static str;

    fn try_from(label: String) -> Result<Self, Self::Error> {
        Self::from_label(&label).ok_or("unsupported version")
    }
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
#[serde(try_from = "String")]
pub enum X12DescriptorEnvelope {
    #[serde(rename = "grouped_transactions")]
    GroupedTransactions,
    #[serde(rename = "mapforce_interchanges")]
    MapForceInterchanges,
}

impl TryFrom<String> for X12DescriptorEnvelope {
    type Error = &'static str;

    fn try_from(label: String) -> Result<Self, Self::Error> {
        match label.as_str() {
            "grouped_transactions" => Ok(Self::GroupedTransactions),
            "mapforce_interchanges" => Ok(Self::MapForceInterchanges),
            _ => Err("unsupported envelope profile"),
        }
    }
}

/// Selected separators. The release character is outside the generated
/// profile and is never written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct X12DescriptorSeparators {
    pub element: char,
    pub component: char,
    pub segment: char,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repetition: Option<char>,
}

impl<'de> Deserialize<'de> for X12DescriptorSeparators {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Members {
            element: char,
            component: char,
            segment: char,
            #[serde(default, deserialize_with = "present")]
            repetition: Option<char>,
        }
        let members: Members = object_only(deserializer)?;
        Ok(Self {
            element: members.element,
            component: members.component,
            segment: members.segment,
            repetition: members.repetition,
        })
    }
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

    #[test]
    fn only_object_and_string_wire_shapes_are_accepted() {
        let base = serde_json::to_value(minimal()).unwrap();
        let positional =
            serde_json::Value::Array(base.as_object().unwrap().values().cloned().collect());
        assert!(serde_json::from_value::<X12Descriptor>(positional).is_err());
        for (field, malformed) in [
            ("version", serde_json::json!({"004010": null})),
            (
                "envelope_profile",
                serde_json::json!({"grouped_transactions": null}),
            ),
            (
                "envelope_profile",
                serde_json::json!({"mapforce_interchanges": null}),
            ),
            ("separators", serde_json::json!(["*", ":", "~", "^"])),
        ] {
            let mut value = base.clone();
            value[field] = malformed;
            assert!(serde_json::from_value::<X12Descriptor>(value).is_err());
        }
    }

    #[test]
    fn repetition_is_optional_but_never_null_and_duplicates_are_refused() {
        for version in ["004010", "005010", "006040"] {
            let mut value = serde_json::to_value(minimal()).unwrap();
            value["version"] = version.into();
            value["separators"] = serde_json::json!({
                "element": "*", "component": ":", "segment": "~",
                // Extra separator keys were ignored by the old reader.
                "extra": true,
            });
            assert!(serde_json::from_value::<X12Descriptor>(value.clone()).is_ok());
            value["separators"]["repetition"] = serde_json::Value::Null;
            assert!(serde_json::from_value::<X12Descriptor>(value.clone()).is_err());
            value["separators"]["repetition"] = "^".into();
            let text = value.to_string();
            assert!(serde_json::from_str::<X12Descriptor>(&text).is_ok());
            for text in [
                text.replacen("{", r#"{"version":"004010","#, 1),
                text.replacen(r#""element":"*""#, r#""element":"*","element":"*""#, 1),
                text.replacen(
                    r#""repetition":"^""#,
                    r#""repetition":"^","repetition":"^""#,
                    1,
                ),
            ] {
                assert!(serde_json::from_str::<X12Descriptor>(&text).is_err());
            }
        }
    }
}
