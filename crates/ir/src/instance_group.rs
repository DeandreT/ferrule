use std::ops::{Deref, DerefMut};

use serde::{Deserialize, Serialize};

use crate::{Instance, XmlTypeOrigin, XmlTypeOriginError};

/// Ordered group data with runtime-only facts owned by this exact occurrence.
///
/// Ordinary construction and deserialization have unknown origin. Exact cloning
/// retains origin, while every mutable field access invalidates it before the
/// caller can change the data. Serialization keeps the legacy ordered array.
///
/// Use `Instance::Group(fields.into())` for ordinary data construction. A field
/// named like historical XML metadata remains data. Full equality includes
/// origin, so a serialized/deserialized known-origin group is data-equivalent
/// but has unknown origin. Compare its serialized data and origin separately.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceGroup {
    fields: Vec<(String, Instance)>,
    #[serde(skip)]
    origin: Option<Box<KnownXmlTypeOrigin>>,
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
        self.origin = match origin {
            XmlTypeOrigin::Unknown => None,
            XmlTypeOrigin::Absent => Some(Box::new(KnownXmlTypeOrigin::Absent)),
            XmlTypeOrigin::Explicit(identity) if !identity.is_empty() => {
                Some(Box::new(KnownXmlTypeOrigin::Explicit(identity.to_owned())))
            }
            XmlTypeOrigin::Explicit(_) => return Err(XmlTypeOriginError::InvalidPayload),
            XmlTypeOrigin::ExplicitPadded {
                literal,
                resolved_identity,
            } if crate::primary_root_padded_type_origin_is_valid(literal, resolved_identity) => {
                Some(Box::new(KnownXmlTypeOrigin::ExplicitPadded {
                    literal: literal.to_owned(),
                    resolved_identity: resolved_identity.to_owned(),
                }))
            }
            XmlTypeOrigin::ExplicitPadded { .. } => return Err(XmlTypeOriginError::InvalidPayload),
        };
        Ok(())
    }

    /// Inspect this exact occurrence without ancestor or collection fallback.
    pub fn xml_type_origin(&self) -> XmlTypeOrigin<'_> {
        match self.origin.as_deref() {
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
            origin: None,
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
        self.origin = None;
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
        self.origin = None;
        self.fields.iter_mut()
    }
}
