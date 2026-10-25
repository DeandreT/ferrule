//! Typed refusals for configurable generated-library identities.
//!
//! Each backend owns the rules for its own identities (C# assembly, namespace
//! and class names; Rust package names) and reports a refusal through
//! [`IdentityError`] before anything is rendered.

use std::fmt;

/// Which configurable identity a refusal concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityKind {
    CSharpAssemblyName,
    CSharpNamespace,
    CSharpMappingClass,
    RustPackageName,
}

impl fmt::Display for IdentityKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CSharpAssemblyName => "C# assembly name",
            Self::CSharpNamespace => "C# root namespace",
            Self::CSharpMappingClass => "C# mapping class name",
            Self::RustPackageName => "Rust package name",
        })
    }
}

/// The rule an identity breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityViolation {
    Empty,
    TooLong {
        maximum: usize,
    },
    /// A dotted name has an empty segment.
    EmptySegment,
    InvalidStart {
        character: char,
    },
    InvalidCharacter {
        character: char,
    },
    /// The name, or one of its segments, is a language keyword.
    Keyword {
        keyword: String,
    },
    /// The name, or one of its segments, is reserved by the platform, the
    /// toolchain or the generated library itself.
    Reserved {
        reserved: String,
    },
    /// A namespace segment would shadow a name the vendored runtime resolves
    /// through its own enclosing namespaces.
    ShadowsRuntimeName {
        segment: String,
    },
    /// The mapping class shares its name with a namespace segment.
    ClassNamesNamespaceSegment,
    /// The name lacks the suffix that keeps it distinct from generated members.
    MissingSuffix {
        suffix: &'static str,
    },
}

impl fmt::Display for IdentityViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("it is empty"),
            Self::TooLong { maximum } => write!(formatter, "it exceeds {maximum} bytes"),
            Self::EmptySegment => formatter.write_str("it has an empty dotted segment"),
            Self::InvalidStart { character } => {
                write!(formatter, "it cannot start a name with {character:?}")
            }
            Self::InvalidCharacter { character } => {
                write!(
                    formatter,
                    "it contains the unsupported character {character:?}"
                )
            }
            Self::Keyword { keyword } => write!(formatter, "{keyword:?} is a language keyword"),
            Self::Reserved { reserved } => write!(formatter, "{reserved:?} is reserved"),
            Self::ShadowsRuntimeName { segment } => write!(
                formatter,
                "segment {segment:?} would shadow a name the vendored runtime uses"
            ),
            Self::ClassNamesNamespaceSegment => {
                formatter.write_str("the class shares its name with a namespace segment")
            }
            Self::MissingSuffix { suffix } => write!(formatter, "it must end with {suffix:?}"),
        }
    }
}

/// A refused generated-library identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityError {
    pub kind: IdentityKind,
    pub value: String,
    pub violation: IdentityViolation,
}

impl IdentityError {
    pub fn new(kind: IdentityKind, value: &str, violation: IdentityViolation) -> Self {
        Self {
            kind,
            value: value.to_owned(),
            violation,
        }
    }
}

impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid generated {} {:?}: {}",
            self.kind, self.value, self.violation
        )
    }
}

impl std::error::Error for IdentityError {}
