use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{FormatOptions, Project, RuntimeBoundary};

/// A native CSV text setting whose physical byte behavior is not implemented.
/// Causes retain no original declaration text or document data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CsvTextRepairCause {
    #[serde(rename = "unsupported_encoding")]
    Encoding,
    #[serde(rename = "unsupported_byte_order")]
    ByteOrder,
    #[serde(rename = "unsupported_byte_order_mark")]
    ByteOrderMark,
}

impl CsvTextRepairCause {
    const fn bit(self) -> u8 {
        match self {
            Self::Encoding => 1,
            Self::ByteOrder => 2,
            Self::ByteOrderMark => 4,
        }
    }
}

impl<'de> Deserialize<'de> for CsvTextRepairCause {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CauseVisitor;
        impl Visitor<'_> for CauseVisitor {
            type Value = CsvTextRepairCause;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a supported CSV text repair cause")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                match value {
                    "unsupported_encoding" => Ok(CsvTextRepairCause::Encoding),
                    "unsupported_byte_order" => Ok(CsvTextRepairCause::ByteOrder),
                    "unsupported_byte_order_mark" => Ok(CsvTextRepairCause::ByteOrderMark),
                    _ => Err(E::custom("unknown CSV text repair cause")),
                }
            }
        }
        deserializer.deserialize_str(CauseVisitor)
    }
}

/// A closed, nonempty set of at most three unsupported native CSV text settings.
/// Kept on repair drafts so save/reopen cannot silently certify guessed byte I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvTextRepairDependency(u8);

impl CsvTextRepairDependency {
    pub const fn new(cause: CsvTextRepairCause) -> Self {
        Self(cause.bit())
    }

    pub const fn with_cause(self, cause: CsvTextRepairCause) -> Self {
        Self(self.0 | cause.bit())
    }

    pub fn causes(self) -> impl Iterator<Item = CsvTextRepairCause> {
        [
            CsvTextRepairCause::Encoding,
            CsvTextRepairCause::ByteOrder,
            CsvTextRepairCause::ByteOrderMark,
        ]
        .into_iter()
        .filter(move |cause| self.0 & cause.bit() != 0)
    }
}

impl Serialize for CsvTextRepairDependency {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("CsvTextRepairDependency", 1)?;
        state.serialize_field("causes", &self.causes().collect::<Vec<_>>())?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for CsvTextRepairDependency {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Causes;
        impl<'de> Deserialize<'de> for Causes {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                // Only used as the object key; unknown keys fail without allocating them.
                struct KeyVisitor;
                impl Visitor<'_> for KeyVisitor {
                    type Value = Causes;
                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str("the causes field")
                    }
                    fn visit_str<E: de::Error>(self, value: &str) -> Result<Causes, E> {
                        if value == "causes" {
                            Ok(Causes)
                        } else {
                            Err(E::custom("unknown CSV text repair field"))
                        }
                    }
                }
                deserializer.deserialize_identifier(KeyVisitor)
            }
        }
        struct CauseSet(CsvTextRepairDependency);
        impl<'de> Deserialize<'de> for CauseSet {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct SetVisitor;
                impl<'de> Visitor<'de> for SetVisitor {
                    type Value = CauseSet;
                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str("one to three distinct CSV text repair causes")
                    }
                    fn visit_seq<A: SeqAccess<'de>>(
                        self,
                        mut sequence: A,
                    ) -> Result<CauseSet, A::Error> {
                        let mut bits = 0_u8;
                        while let Some(cause) = sequence.next_element::<CsvTextRepairCause>()? {
                            if bits & cause.bit() != 0 {
                                return Err(de::Error::custom("duplicate CSV text repair cause"));
                            }
                            bits |= cause.bit();
                        }
                        if bits == 0 {
                            return Err(de::Error::custom(
                                "CSV text repair causes must not be empty",
                            ));
                        }
                        Ok(CauseSet(CsvTextRepairDependency(bits)))
                    }
                }
                deserializer.deserialize_seq(SetVisitor)
            }
        }
        struct DependencyVisitor;
        impl<'de> Visitor<'de> for DependencyVisitor {
            type Value = CsvTextRepairDependency;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a CSV text repair object with a causes field")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut causes = None;
                while map.next_key::<Causes>()?.is_some() {
                    if causes.is_some() {
                        return Err(de::Error::duplicate_field("causes"));
                    }
                    causes = Some(map.next_value::<CauseSet>()?.0);
                }
                causes.ok_or_else(|| de::Error::missing_field("causes"))
            }
        }
        deserializer.deserialize_map(DependencyVisitor)
    }
}

impl fmt::Display for CsvTextRepairDependency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unsupported native CSV text settings (")?;
        for (index, cause) in self.causes().enumerate() {
            if index != 0 {
                formatter.write_str(", ")?;
            }
            formatter.write_str(match cause {
                CsvTextRepairCause::Encoding => "encoding",
                CsvTextRepairCause::ByteOrder => "byte order",
                CsvTextRepairCause::ByteOrderMark => "byte order mark",
            })?;
        }
        formatter.write_str("); the stored CSV boundary is a repair draft")
    }
}

/// One configured boundary that cannot faithfully decode or encode CSV bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvRuntimeDependency {
    pub boundary: RuntimeBoundary,
    pub dependency: CsvTextRepairDependency,
}

impl fmt::Display for CsvRuntimeDependency {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} requires {}", self.boundary, self.dependency)
    }
}

impl Project {
    /// Reports persisted CSV byte-I/O blockers without reopening native designs.
    /// Deliberately host-preparsed typed instances remain usable with the engine.
    pub fn csv_runtime_dependencies(&self) -> Vec<CsvRuntimeDependency> {
        let mut dependencies = Vec::new();
        collect(
            &mut dependencies,
            RuntimeBoundary::PrimarySource,
            &self.source_options,
        );
        collect(
            &mut dependencies,
            RuntimeBoundary::PrimaryTarget,
            &self.target_options,
        );
        for source in &self.extra_sources {
            collect(
                &mut dependencies,
                RuntimeBoundary::NamedSource(source.name.clone()),
                &source.options,
            );
        }
        for target in &self.extra_targets {
            collect(
                &mut dependencies,
                RuntimeBoundary::NamedTarget(target.name.clone()),
                &target.options,
            );
        }
        dependencies
    }
}

fn collect(
    dependencies: &mut Vec<CsvRuntimeDependency>,
    boundary: RuntimeBoundary,
    options: &FormatOptions,
) {
    if let Some(dependency) = options.csv_text_repair_dependency {
        dependencies.push(CsvRuntimeDependency {
            boundary,
            dependency,
        });
    }
}
