use std::ops::{Deref, DerefMut};

use serde::{Deserialize, Serialize};

use crate::{Instance, SourceRowOrigin, XmlTypeOrigin, XmlTypeOriginError};

/// Ordered group data with runtime-only facts owned by this exact occurrence.
///
/// Ordinary construction and deserialization have unknown origin. Exact cloning
/// retains origin, while every mutable field access invalidates it before the
/// caller can change the data. Serialization keeps the legacy ordered array.
///
/// Use `Instance::Group(fields.into())` for ordinary data construction. A field
/// named like historical XML metadata remains data. Equality includes XML type
/// facts but excludes physical source provenance, which has separate identity
/// comparisons. Serialization never includes either kind of runtime metadata.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceGroup {
    fields: Vec<(String, Instance)>,
    #[serde(skip)]
    origins: Option<Box<GroupOrigins>>,
}

#[derive(Clone, Default)]
struct GroupOrigins {
    xml: Option<KnownXmlTypeOrigin>,
    source: Option<SourceRowOrigin>,
}

#[derive(Clone, PartialEq)]
enum KnownXmlTypeOrigin {
    Absent,
    Explicit(String),
    ExplicitPadded {
        literal: String,
        resolved_identity: String,
    },
}

impl InstanceGroup {
    /// Provenance is separate from ordinary value equality and is never saved.
    pub fn source_origin(&self) -> Option<&SourceRowOrigin> {
        self.origins.as_deref()?.source.as_ref()
    }

    /// Attach reader/host provenance without changing the row's ordered data.
    pub fn with_source_origin(mut self, origin: SourceRowOrigin) -> Self {
        self.origins.get_or_insert_with(Default::default).source = Some(origin);
        self
    }

    /// Attach a fact from the owning XML reader or a trusted host boundary.
    /// This is not an attestation mechanism for arbitrary programmatic callers.
    pub fn with_xml_type_origin(
        mut self,
        origin: XmlTypeOrigin<'_>,
    ) -> Result<Self, XmlTypeOriginError> {
        self.set_xml_type_origin(origin)?;
        Ok(self)
    }

    /// Attach an independently resolved source annotation, never a data field.
    pub fn set_xml_type_origin(
        &mut self,
        origin: XmlTypeOrigin<'_>,
    ) -> Result<(), XmlTypeOriginError> {
        let known = match origin {
            XmlTypeOrigin::Unknown => None,
            XmlTypeOrigin::Absent => Some(KnownXmlTypeOrigin::Absent),
            XmlTypeOrigin::Explicit(identity) if !identity.is_empty() => {
                Some(KnownXmlTypeOrigin::Explicit(identity.to_owned()))
            }
            XmlTypeOrigin::Explicit(_) => return Err(XmlTypeOriginError::InvalidPayload),
            XmlTypeOrigin::ExplicitPadded {
                literal,
                resolved_identity,
            } if crate::primary_root_padded_type_origin_is_valid(literal, resolved_identity) => {
                Some(KnownXmlTypeOrigin::ExplicitPadded {
                    literal: literal.to_owned(),
                    resolved_identity: resolved_identity.to_owned(),
                })
            }
            XmlTypeOrigin::ExplicitPadded { .. } => return Err(XmlTypeOriginError::InvalidPayload),
        };
        if known.is_some() || self.origins.is_some() {
            let origins = self.origins.get_or_insert_with(Default::default);
            origins.xml = known;
            if origins.xml.is_none() && origins.source.is_none() {
                self.origins = None;
            }
        }
        Ok(())
    }

    /// Inspect this exact occurrence without ancestor or collection fallback.
    pub fn xml_type_origin(&self) -> XmlTypeOrigin<'_> {
        match self
            .origins
            .as_deref()
            .and_then(|origins| origins.xml.as_ref())
        {
            None => XmlTypeOrigin::Unknown,
            Some(KnownXmlTypeOrigin::Absent) => XmlTypeOrigin::Absent,
            Some(KnownXmlTypeOrigin::Explicit(identity)) => XmlTypeOrigin::Explicit(identity),
            Some(KnownXmlTypeOrigin::ExplicitPadded {
                literal,
                resolved_identity,
            }) => XmlTypeOrigin::ExplicitPadded {
                literal,
                resolved_identity,
            },
        }
    }

    /// Consume only the ordered data; any reconstruction has unknown origin.
    pub fn into_fields(self) -> Vec<(String, Instance)> {
        self.fields
    }
}

impl From<Vec<(String, Instance)>> for InstanceGroup {
    fn from(fields: Vec<(String, Instance)>) -> Self {
        Self {
            fields,
            origins: None,
        }
    }
}

impl Deref for InstanceGroup {
    type Target = Vec<(String, Instance)>;

    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

impl DerefMut for InstanceGroup {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.origins = None;
        &mut self.fields
    }
}

impl std::fmt::Debug for InstanceGroup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.fields, formatter)
    }
}

impl IntoIterator for InstanceGroup {
    type Item = (String, Instance);
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.fields.into_iter()
    }
}

impl<'a> IntoIterator for &'a InstanceGroup {
    type Item = &'a (String, Instance);
    type IntoIter = std::slice::Iter<'a, (String, Instance)>;

    fn into_iter(self) -> Self::IntoIter {
        self.fields.iter()
    }
}

impl<'a> IntoIterator for &'a mut InstanceGroup {
    type Item = &'a mut (String, Instance);
    type IntoIter = std::slice::IterMut<'a, (String, Instance)>;

    fn into_iter(self) -> Self::IntoIter {
        self.origins = None;
        self.fields.iter_mut()
    }
}

impl PartialEq for InstanceGroup {
    fn eq(&self, other: &Self) -> bool {
        // Keep the pre-existing value/XML-origin equality contract. Physical
        // row identity has its own explicit same_occurrence comparison.
        self.fields == other.fields
            && self
                .origins
                .as_deref()
                .and_then(|origins| origins.xml.as_ref())
                == other
                    .origins
                    .as_deref()
                    .and_then(|origins| origins.xml.as_ref())
    }
}
