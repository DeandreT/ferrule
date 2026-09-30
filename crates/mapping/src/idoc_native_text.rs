//! Certified provenance for the one observed native IDoc text-settings grammar.
//!
//! The numeric codes are deliberately uninterpreted. This model preserves a
//! native design's settings without claiming that Ferrule's IDoc byte I/O or
//! validation actions behave like the reference application.

use serde::{Deserialize, Deserializer, Serialize};

pub const IDOC_NATIVE_VALIDATION_CASES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdocNativeTextError {
    Invalid(&'static str),
}

impl std::fmt::Display for IdocNativeTextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(reason) => {
                write!(formatter, "invalid native IDoc text settings: {reason}")
            }
        }
    }
}

impl std::error::Error for IdocNativeTextError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdocNativeValidationKind {
    MissingSegment,
    MissingGroup,
    MissingFieldOrComposite,
    ExtraData,
    InvalidFieldValue,
    InvalidDate,
    InvalidTime,
    ExtraRepeat,
    NumericOverflow,
    DataElementTooShort,
    DataElementTooLong,
    UnexpectedEof,
    InvalidCodelistValue,
    Semantic,
    SegmentNotInMessage,
    UnrecognizedSegmentId,
}

impl IdocNativeValidationKind {
    pub const ALL: [Self; IDOC_NATIVE_VALIDATION_CASES] = [
        Self::MissingSegment,
        Self::MissingGroup,
        Self::MissingFieldOrComposite,
        Self::ExtraData,
        Self::InvalidFieldValue,
        Self::InvalidDate,
        Self::InvalidTime,
        Self::ExtraRepeat,
        Self::NumericOverflow,
        Self::DataElementTooShort,
        Self::DataElementTooLong,
        Self::UnexpectedEof,
        Self::InvalidCodelistValue,
        Self::Semantic,
        Self::SegmentNotInMessage,
        Self::UnrecognizedSegmentId,
    ];

    pub const fn native_name(self) -> &'static str {
        match self {
            Self::MissingSegment => "missing-segment",
            Self::MissingGroup => "missing-group",
            Self::MissingFieldOrComposite => "missing-field-or-composite",
            Self::ExtraData => "extra-data",
            Self::InvalidFieldValue => "invalid-field-value",
            Self::InvalidDate => "invalid-date",
            Self::InvalidTime => "invalid-time",
            Self::ExtraRepeat => "extra-repeat",
            Self::NumericOverflow => "numeric-overflow",
            Self::DataElementTooShort => "data-element-too-short",
            Self::DataElementTooLong => "data-element-too-long",
            Self::UnexpectedEof => "unexpected-eof",
            Self::InvalidCodelistValue => "invalid-codelist-value",
            Self::Semantic => "semantic",
            Self::SegmentNotInMessage => "segment-not-in-message",
            Self::UnrecognizedSegmentId => "unrecognized-segment-id",
        }
    }

    pub fn from_native_name(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.native_name() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdocNativeValidationAction {
    #[serde(rename = "stop")]
    Stop,
    #[serde(rename = "report+reject")]
    ReportReject,
}

impl IdocNativeValidationAction {
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::ReportReject => "report+reject",
        }
    }

    pub fn from_native_name(value: &str) -> Option<Self> {
        match value {
            "stop" => Some(Self::Stop),
            "report+reject" => Some(Self::ReportReject),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdocNativeValidationCase {
    kind: IdocNativeValidationKind,
    action: IdocNativeValidationAction,
}

impl IdocNativeValidationCase {
    pub const fn new(kind: IdocNativeValidationKind, action: IdocNativeValidationAction) -> Self {
        Self { kind, action }
    }

    pub const fn kind(self) -> IdocNativeValidationKind {
        self.kind
    }

    pub const fn action(self) -> IdocNativeValidationAction {
        self.action
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct IdocNativeSeparators {
    data_element: u8,
    component: u8,
    decimal: u8,
    escape: u8,
    repetition: u8,
    segment: u8,
    subcomponent: Option<u8>,
}

impl IdocNativeSeparators {
    const OBSERVED_FIXED: Self = Self {
        data_element: b' ',
        component: b' ',
        decimal: b'.',
        escape: b' ',
        repetition: b' ',
        segment: b'\n',
        subcomponent: None,
    };

    pub const fn data_element(self) -> u8 {
        self.data_element
    }

    pub const fn component(self) -> u8 {
        self.component
    }

    pub const fn decimal(self) -> u8 {
        self.decimal
    }

    pub const fn escape(self) -> u8 {
        self.escape
    }

    pub const fn repetition(self) -> u8 {
        self.repetition
    }

    pub const fn segment(self) -> u8 {
        self.segment
    }

    pub const fn subcomponent(self) -> Option<u8> {
        self.subcomponent
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeparatorsWire {
    data_element: u8,
    component: u8,
    decimal: u8,
    escape: u8,
    repetition: u8,
    segment: u8,
    #[serde(deserialize_with = "deserialize_optional_byte")]
    subcomponent: Option<u8>,
}

fn deserialize_optional_byte<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<u8>::deserialize(deserializer)
}

impl SeparatorsWire {
    fn is_observed_fixed(&self) -> bool {
        self.data_element == b' '
            && self.component == b' '
            && self.decimal == b'.'
            && self.escape == b' '
            && self.repetition == b' '
            && self.segment == b'\n'
            && self.subcomponent.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum NativeSyntaxLevel {
    #[serde(rename = "A")]
    A,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum NativeControllingAgency {
    #[serde(rename = "Fixed")]
    Fixed,
}

/// Exact native text settings known to be present in the local IDoc design.
/// Codes `1` and `0` are identities, not interpreted character-set claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdocNativeTextSettings {
    encoding_code: u8,
    byte_order_code: u8,
    byte_order_mark_code: u8,
    unpacked_format: bool,
    autocomplete_data: bool,
    terminate_with_line_feed: bool,
    syntax_version_number: u8,
    controlling_agency: NativeControllingAgency,
    syntax_level: NativeSyntaxLevel,
    is_idoc: bool,
    separators: IdocNativeSeparators,
    validation_cases: Vec<IdocNativeValidationCase>,
}

impl IdocNativeTextSettings {
    pub fn new_observed_profile(
        unpacked_format: bool,
        autocomplete_data: bool,
        terminate_with_line_feed: bool,
        validation_cases: Vec<IdocNativeValidationCase>,
    ) -> Result<Self, IdocNativeTextError> {
        validate_cases(&validation_cases)?;
        Ok(Self {
            encoding_code: 1,
            byte_order_code: 1,
            byte_order_mark_code: 0,
            unpacked_format,
            autocomplete_data,
            terminate_with_line_feed,
            syntax_version_number: 2,
            controlling_agency: NativeControllingAgency::Fixed,
            syntax_level: NativeSyntaxLevel::A,
            is_idoc: true,
            separators: IdocNativeSeparators::OBSERVED_FIXED,
            validation_cases,
        })
    }

    pub const fn encoding_code(&self) -> u8 {
        self.encoding_code
    }

    pub const fn byte_order_code(&self) -> u8 {
        self.byte_order_code
    }

    pub const fn byte_order_mark_code(&self) -> u8 {
        self.byte_order_mark_code
    }

    pub const fn unpacked_format(&self) -> bool {
        self.unpacked_format
    }

    pub const fn autocomplete_data(&self) -> bool {
        self.autocomplete_data
    }

    pub const fn terminate_with_line_feed(&self) -> bool {
        self.terminate_with_line_feed
    }

    pub const fn syntax_version_number(&self) -> u8 {
        self.syntax_version_number
    }

    pub const fn controlling_agency(&self) -> &'static str {
        "Fixed"
    }

    pub const fn syntax_level(&self) -> &'static str {
        "A"
    }

    pub const fn is_idoc(&self) -> bool {
        self.is_idoc
    }

    pub const fn separators(&self) -> IdocNativeSeparators {
        self.separators
    }

    pub fn validation_cases(&self) -> &[IdocNativeValidationCase] {
        &self.validation_cases
    }
}

fn validate_cases(cases: &[IdocNativeValidationCase]) -> Result<(), IdocNativeTextError> {
    if cases.len() != IDOC_NATIVE_VALIDATION_CASES {
        return Err(IdocNativeTextError::Invalid(
            "the validation matrix must contain all 16 known cases",
        ));
    }
    let mut found = 0_u16;
    for case in cases {
        let index = IdocNativeValidationKind::ALL
            .iter()
            .position(|kind| *kind == case.kind)
            .ok_or(IdocNativeTextError::Invalid("unknown validation case"))?;
        let bit = 1_u16 << index;
        if found & bit != 0 {
            return Err(IdocNativeTextError::Invalid("duplicate validation case"));
        }
        found |= bit;
    }
    Ok(())
}

fn deserialize_cases<'de, D>(deserializer: D) -> Result<Vec<IdocNativeValidationCase>, D::Error>
where
    D: Deserializer<'de>,
{
    struct CasesVisitor;

    impl<'de> serde::de::Visitor<'de> for CasesVisitor {
        type Value = Vec<IdocNativeValidationCase>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("at most 16 IDoc validation cases")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut cases = Vec::with_capacity(IDOC_NATIVE_VALIDATION_CASES);
            while let Some(case) = sequence.next_element()? {
                if cases.len() == IDOC_NATIVE_VALIDATION_CASES {
                    return Err(serde::de::Error::custom(
                        "IDoc validation matrix exceeds 16 cases",
                    ));
                }
                cases.push(case);
            }
            Ok(cases)
        }
    }

    deserializer.deserialize_seq(CasesVisitor)
}

impl<'de> Deserialize<'de> for IdocNativeTextSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            encoding_code: u8,
            byte_order_code: u8,
            byte_order_mark_code: u8,
            unpacked_format: bool,
            autocomplete_data: bool,
            terminate_with_line_feed: bool,
            syntax_version_number: u8,
            controlling_agency: NativeControllingAgency,
            syntax_level: NativeSyntaxLevel,
            is_idoc: bool,
            separators: SeparatorsWire,
            #[serde(deserialize_with = "deserialize_cases")]
            validation_cases: Vec<IdocNativeValidationCase>,
        }

        let repr = Repr::deserialize(deserializer)?;
        if repr.encoding_code != 1
            || repr.byte_order_code != 1
            || repr.byte_order_mark_code != 0
            || repr.syntax_version_number != 2
            || !repr.is_idoc
            || !repr.separators.is_observed_fixed()
        {
            return Err(serde::de::Error::custom(
                "unsupported native IDoc text code or separator profile",
            ));
        }
        let _ = (repr.controlling_agency, repr.syntax_level);
        Self::new_observed_profile(
            repr.unpacked_format,
            repr.autocomplete_data,
            repr.terminate_with_line_feed,
            repr.validation_cases,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete_cases() -> Vec<IdocNativeValidationCase> {
        IdocNativeValidationKind::ALL
            .into_iter()
            .map(|kind| IdocNativeValidationCase::new(kind, IdocNativeValidationAction::Stop))
            .collect()
    }

    #[test]
    fn exact_profile_roundtrips_and_rejects_tampered_codes() {
        let settings =
            IdocNativeTextSettings::new_observed_profile(false, true, false, complete_cases())
                .unwrap();
        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            serde_json::from_value::<IdocNativeTextSettings>(value.clone()).unwrap(),
            settings
        );
        let mut altered = value.clone();
        altered["encoding_code"] = serde_json::json!(52);
        assert!(serde_json::from_value::<IdocNativeTextSettings>(altered).is_err());
        let mut altered = value.clone();
        altered["separators"]["segment"] = serde_json::json!(13);
        assert!(serde_json::from_value::<IdocNativeTextSettings>(altered).is_err());
        let mut altered = value.clone();
        altered["separators"]
            .as_object_mut()
            .unwrap()
            .remove("subcomponent");
        assert!(serde_json::from_value::<IdocNativeTextSettings>(altered).is_err());
        let mut altered = value.clone();
        let extra = altered["validation_cases"][0].clone();
        altered["validation_cases"]
            .as_array_mut()
            .unwrap()
            .push(extra);
        assert!(serde_json::from_value::<IdocNativeTextSettings>(altered).is_err());
        let mut altered = value;
        altered["validation_cases"][1]["kind"] = serde_json::json!("missing-segment");
        assert!(serde_json::from_value::<IdocNativeTextSettings>(altered).is_err());
    }

    #[test]
    fn validation_matrix_requires_every_known_case_exactly_once() {
        let mut cases = complete_cases();
        cases.pop();
        assert!(IdocNativeTextSettings::new_observed_profile(false, true, false, cases).is_err());
        let mut cases = complete_cases();
        cases[1] = cases[0];
        assert!(IdocNativeTextSettings::new_observed_profile(false, true, false, cases).is_err());
    }
}
