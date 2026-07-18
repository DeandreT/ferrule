use serde::{Deserialize, Serialize};

/// Runtime EDI family owned by a mapping document boundary.
///
/// The compiled schema remains format-agnostic, so this discriminator is
/// retained separately to reproduce the original component family on export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdiBoundaryKind {
    X12,
    Edifact,
    Hl7,
    Tradacoms,
    Idoc,
    SwiftMt,
}

/// Runtime separators retained from an ANSI X12 document boundary.
///
/// X12 interchanges declare most syntax characters in the ISA envelope, but
/// a mapping target needs them before that envelope exists. Some mapping
/// configurations also declare a release character, which is not discoverable
/// from ISA and must travel with the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct X12Separators {
    pub element: char,
    pub component: char,
    pub segment: char,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition: Option<char>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<char>,
}
