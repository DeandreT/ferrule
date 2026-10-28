//! Explicit XML-only output semantics, separate from the declared schema.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{ScalarType, SchemaKind, SchemaNode};

pub const MAX_XML_SCALAR_OUTPUT_FIELDS: usize = 256;
pub const MAX_XML_SCALAR_OUTPUT_NAME_BYTES: usize = 256;
pub const MAX_XML_SCALAR_OUTPUT_NAMES_BYTES: usize = 16 * 1024;
pub const MAX_XML_SCALAR_OUTPUT_DETAIL_BYTES: usize = 1024;
/// Bounds for the document-wide profile's declared target tree.
pub const MAX_XML_SCALAR_OUTPUT_DOCUMENT_NODES: usize = 1024;
pub const MAX_XML_SCALAR_OUTPUT_DOCUMENT_DEPTH: usize = 32;

/// A known request that is retained for repair rather than executed approximately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XmlScalarOutputUnresolvedReason {
    SchemaUnavailable,
    UnsupportedShape,
    UnsupportedOverride,
    AmbiguousDeclaration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum XmlScalarOutputPolicy {
    /// One flat group whose retained `xs:double` declarations export exactly.
    SuppressCastInSubtreeV1 {
        declarations: XmlDoubleFields,
    },
    /// Every scalar binding of the ordinary primary document keeps its mapped
    /// value. No declared types are retained beyond the target schema.
    SuppressCastInDocumentV1 {},
    Unresolved {
        request: XmlScalarOutputUnresolved,
    },
}

/// Which target bindings skip scalar adaptation under a validated policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlScalarPreservation<'a> {
    /// Only these fields of the root group.
    RootFields(&'a XmlDoubleFields),
    /// Every scalar binding of every scope in the primary document.
    Document,
}

impl XmlScalarPreservation<'_> {
    pub fn preserves(self, field: &str) -> bool {
        match self {
            Self::RootFields(fields) => fields.contains(field),
            Self::Document => true,
        }
    }

    /// The preservation a child scope inherits.
    pub fn for_child(self) -> Option<Self> {
        match self {
            Self::RootFields(_) => None,
            Self::Document => Some(self),
        }
    }
}

/// Complete, canonical declaration list for the flat double fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct XmlDoubleFields(Vec<String>);

impl XmlDoubleFields {
    pub fn new(fields: Vec<String>) -> Result<Self, XmlScalarOutputError> {
        if fields.is_empty()
            || fields.len() > MAX_XML_SCALAR_OUTPUT_FIELDS
            || fields.iter().map(String::len).sum::<usize>() > MAX_XML_SCALAR_OUTPUT_NAMES_BYTES
            || !fields.iter().all(|name| valid_name(name))
            || !fields.is_sorted_by(|left, right| left < right)
        {
            return Err(XmlScalarOutputError::InvalidDeclarations);
        }
        Ok(Self(fields))
    }

    pub fn names(&self) -> &[String] {
        &self.0
    }

    pub fn contains(&self, name: &str) -> bool {
        self.0
            .binary_search_by(|field| field.as_str().cmp(name))
            .is_ok()
    }
}

impl<'de> Deserialize<'de> for XmlDoubleFields {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Fields;
        impl<'de> serde::de::Visitor<'de> for Fields {
            type Value = XmlDoubleFields;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a bounded sorted array of XML field names")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                use serde::de::Error as _;
                let mut fields = Vec::<String>::new();
                let mut bytes = 0;
                while let Some(field) = seq.next_element::<String>()? {
                    if fields.len() == MAX_XML_SCALAR_OUTPUT_FIELDS
                        || !valid_name(&field)
                        || fields.last().is_some_and(|previous| previous >= &field)
                        || bytes + field.len() > MAX_XML_SCALAR_OUTPUT_NAMES_BYTES
                    {
                        return Err(A::Error::custom(XmlScalarOutputError::InvalidDeclarations));
                    }
                    bytes += field.len();
                    fields.push(field);
                }
                XmlDoubleFields::new(fields).map_err(A::Error::custom)
            }
        }
        deserializer.deserialize_seq(Fields)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct XmlScalarOutputUnresolved {
    reason: XmlScalarOutputUnresolvedReason,
    detail: String,
}

impl XmlScalarOutputUnresolved {
    pub fn for_reason(reason: XmlScalarOutputUnresolvedReason) -> Self {
        let detail = match reason {
            XmlScalarOutputUnresolvedReason::SchemaUnavailable => {
                "authoritative schema is unavailable"
            }
            XmlScalarOutputUnresolvedReason::UnsupportedShape => {
                "target shape is outside the supported profile"
            }
            XmlScalarOutputUnresolvedReason::UnsupportedOverride => {
                "cast settings contain an unsupported override"
            }
            XmlScalarOutputUnresolvedReason::AmbiguousDeclaration => {
                "target declarations are ambiguous"
            }
        };
        Self {
            reason,
            detail: detail.into(),
        }
    }
    pub fn new(
        reason: XmlScalarOutputUnresolvedReason,
        detail: String,
    ) -> Result<Self, XmlScalarOutputError> {
        if detail.is_empty()
            || detail.len() > MAX_XML_SCALAR_OUTPUT_DETAIL_BYTES
            || detail.chars().any(char::is_control)
        {
            return Err(XmlScalarOutputError::InvalidDetail);
        }
        Ok(Self { reason, detail })
    }
    pub fn reason(&self) -> XmlScalarOutputUnresolvedReason {
        self.reason
    }
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl<'de> Deserialize<'de> for XmlScalarOutputUnresolved {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            reason: XmlScalarOutputUnresolvedReason,
            detail: String,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.reason, wire.detail).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlScalarOutputError {
    InvalidDeclarations,
    InvalidDetail,
    UnsupportedSchema,
    DeclarationMismatch,
    Unresolved,
}

impl fmt::Display for XmlScalarOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidDeclarations => "XML scalar output requires a nonempty, bounded, sorted set of unique flat field names",
            Self::InvalidDetail => "unresolved XML scalar output detail is empty, too long, or contains control characters",
            Self::UnsupportedSchema => "XML scalar output requires a closed unqualified plain element schema without constraints",
            Self::DeclarationMismatch => "XML scalar output declarations must identify every Float field exactly once",
            Self::Unresolved => "explicit XML scalar output request is unresolved",
        })
    }
}
impl std::error::Error for XmlScalarOutputError {}

impl XmlScalarOutputPolicy {
    pub fn validate_schema(
        &self,
        schema: &SchemaNode,
    ) -> Result<XmlScalarPreservation<'_>, XmlScalarOutputError> {
        let declarations = match self {
            Self::SuppressCastInSubtreeV1 { declarations } => declarations,
            Self::SuppressCastInDocumentV1 {} => {
                let mut nodes = 0;
                validate_document_node(schema, 0, &mut nodes)?;
                if !matches!(schema.kind, SchemaKind::Group { .. })
                    || schema.repeating
                    || schema.xml_optional
                {
                    return Err(XmlScalarOutputError::UnsupportedSchema);
                }
                return Ok(XmlScalarPreservation::Document);
            }
            Self::Unresolved { .. } => return Err(XmlScalarOutputError::Unresolved),
        };
        let SchemaKind::Group { children, .. } = &schema.kind else {
            return Err(XmlScalarOutputError::UnsupportedSchema);
        };
        if !valid_name(&schema.name) || children.len() > MAX_XML_SCALAR_OUTPUT_FIELDS {
            return Err(XmlScalarOutputError::UnsupportedSchema);
        }
        let mut expected_children = Vec::with_capacity(children.len());
        let mut names = BTreeSet::new();
        let mut doubles = BTreeSet::new();
        for child in children {
            if !valid_name(&child.name) || !names.insert(child.name.as_str()) {
                return Err(XmlScalarOutputError::UnsupportedSchema);
            }
            let SchemaKind::Scalar {
                ty: ty @ (ScalarType::String | ScalarType::Float),
            } = child.kind
            else {
                return Err(XmlScalarOutputError::UnsupportedSchema);
            };
            let mut expected = SchemaNode::scalar(&child.name, ty);
            expected.repeating = child.repeating;
            expected.xml_optional = child.xml_optional;
            expected.xml_namespace = child.xml_namespace.clone();
            if child != &expected
                || child
                    .xml_namespace
                    .as_ref()
                    .is_some_and(|namespace| !matches!(namespace, crate::XmlNamespace::Unqualified))
            {
                return Err(XmlScalarOutputError::UnsupportedSchema);
            }
            if ty == ScalarType::Float {
                doubles.insert(child.name.as_str());
            }
            expected_children.push(expected);
        }
        let mut expected = SchemaNode::group(&schema.name, expected_children);
        expected.xml_namespace = schema.xml_namespace.clone();
        if schema != &expected
            || schema
                .xml_namespace
                .as_ref()
                .is_some_and(|namespace| !matches!(namespace, crate::XmlNamespace::Unqualified))
        {
            return Err(XmlScalarOutputError::UnsupportedSchema);
        }
        if doubles
            .iter()
            .copied()
            .ne(declarations.names().iter().map(String::as_str))
        {
            return Err(XmlScalarOutputError::DeclarationMismatch);
        }
        Ok(XmlScalarPreservation::RootFields(declarations))
    }
}

/// A plain, unqualified element tree: the same shape every ordinary XML
/// writer accepts, without attributes, choices, wildcards or nil values.
fn validate_document_node(
    node: &SchemaNode,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), XmlScalarOutputError> {
    *nodes += 1;
    if depth > MAX_XML_SCALAR_OUTPUT_DOCUMENT_DEPTH
        || *nodes > MAX_XML_SCALAR_OUTPUT_DOCUMENT_NODES
        || !valid_name(&node.name)
        || node
            .xml_namespace
            .as_ref()
            .is_some_and(|namespace| !matches!(namespace, crate::XmlNamespace::Unqualified))
    {
        return Err(XmlScalarOutputError::UnsupportedSchema);
    }
    let mut expected = match &node.kind {
        SchemaKind::Scalar { ty } => SchemaNode::scalar(&node.name, *ty),
        SchemaKind::Group { children, .. } => {
            let mut names = BTreeSet::new();
            for child in children {
                if !names.insert(child.name.as_str()) {
                    return Err(XmlScalarOutputError::UnsupportedSchema);
                }
                validate_document_node(child, depth + 1, nodes)?;
            }
            SchemaNode::group(&node.name, children.clone())
        }
        _ => return Err(XmlScalarOutputError::UnsupportedSchema),
    };
    expected.repeating = node.repeating;
    expected.xml_optional = node.xml_optional;
    expected.xml_namespace = node.xml_namespace.clone();
    if node != &expected {
        return Err(XmlScalarOutputError::UnsupportedSchema);
    }
    Ok(())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_XML_SCALAR_OUTPUT_NAME_BYTES
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}
