//! XSD 1.1 wildcard exclusions: exact expanded names a wildcard never matches.
//!
//! Import resolves every `notQName` token, including the `##defined` and
//! `##definedSibling` keywords, to this closed set. The keywords themselves
//! are never retained.

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{SchemaKind, SchemaNode, XmlNamespace, XmlWildcardNamespaceConstraint};

/// Maximum number of excluded names one wildcard may retain.
pub const MAX_XML_WILDCARD_EXCLUDED_NAMES: usize = 256;

/// One exact XML expanded name whose local part is an NCName.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct XmlExpandedName {
    namespace: XmlNamespace,
    local: String,
}

impl XmlExpandedName {
    pub fn new(namespace: XmlNamespace, local: impl Into<String>) -> Option<Self> {
        let local = local.into();
        crate::primary_root_ncname_is_valid(&local).then_some(Self { namespace, local })
    }

    pub fn namespace(&self) -> &XmlNamespace {
        &self.namespace
    }

    pub fn local(&self) -> &str {
        &self.local
    }

    /// Unqualified names first, then namespace URI bytes, then local-name bytes.
    fn canonical_cmp(&self, other: &Self) -> Ordering {
        (self.namespace.uri(), self.local.as_str())
            .cmp(&(other.namespace.uri(), other.local.as_str()))
    }
}

impl<'de> Deserialize<'de> for XmlExpandedName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Repr {
            namespace: XmlNamespace,
            local: String,
        }

        let repr = Repr::deserialize(deserializer)?;
        Self::new(repr.namespace, repr.local).ok_or_else(|| {
            serde::de::Error::custom("an XML expanded name requires an NCName local part")
        })
    }
}

/// A nonempty, duplicate-free set of XML expanded names in canonical order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct XmlWildcardExcludedNames(Vec<XmlExpandedName>);

impl XmlWildcardExcludedNames {
    /// Sorts `names` canonically. Duplicates are refused rather than merged.
    pub fn new(
        names: impl IntoIterator<Item = XmlExpandedName>,
    ) -> Result<Self, XmlWildcardExcludedNamesError> {
        let mut names = names
            .into_iter()
            .take(MAX_XML_WILDCARD_EXCLUDED_NAMES + 1)
            .collect::<Vec<_>>();
        if names.len() > MAX_XML_WILDCARD_EXCLUDED_NAMES {
            return Err(XmlWildcardExcludedNamesError::TooMany);
        }
        names.sort_by(XmlExpandedName::canonical_cmp);
        if names.is_empty() {
            return Err(XmlWildcardExcludedNamesError::Empty);
        }
        if names
            .windows(2)
            .any(|pair| pair[0].canonical_cmp(&pair[1]) == Ordering::Equal)
        {
            return Err(XmlWildcardExcludedNamesError::Duplicate);
        }
        Ok(Self(names))
    }

    pub fn iter(&self) -> impl Iterator<Item = &XmlExpandedName> {
        self.0.iter()
    }
}

impl<'de> Deserialize<'de> for XmlWildcardExcludedNames {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Names;

        impl<'de> serde::de::Visitor<'de> for Names {
            type Value = XmlWildcardExcludedNames;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a bounded canonical array of XML expanded names")
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                use serde::de::Error as _;
                let mut names = Vec::<XmlExpandedName>::new();
                while let Some(name) = seq.next_element::<XmlExpandedName>()? {
                    if names.len() == MAX_XML_WILDCARD_EXCLUDED_NAMES {
                        return Err(A::Error::custom(XmlWildcardExcludedNamesError::TooMany));
                    }
                    match names.last().map(|previous| previous.canonical_cmp(&name)) {
                        Some(Ordering::Equal) => {
                            return Err(A::Error::custom(XmlWildcardExcludedNamesError::Duplicate));
                        }
                        Some(Ordering::Greater) => {
                            return Err(A::Error::custom(
                                XmlWildcardExcludedNamesError::NonCanonicalOrder,
                            ));
                        }
                        Some(Ordering::Less) | None => names.push(name),
                    }
                }
                XmlWildcardExcludedNames::new(names).map_err(A::Error::custom)
            }
        }

        deserializer.deserialize_seq(Names)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XmlWildcardExcludedNamesError {
    Empty,
    TooMany,
    Duplicate,
    /// Serialized sets must already be in canonical order.
    NonCanonicalOrder,
}

impl fmt::Display for XmlWildcardExcludedNamesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("XML wildcard excluded names cannot be empty"),
            Self::TooMany => write!(
                formatter,
                "XML wildcard excluded names exceed the {MAX_XML_WILDCARD_EXCLUDED_NAMES}-name limit"
            ),
            Self::Duplicate => formatter.write_str("XML wildcard excluded names contain a duplicate"),
            Self::NonCanonicalOrder => formatter.write_str(
                "XML wildcard excluded names must be sorted: unqualified first, then namespace URI, then local name",
            ),
        }
    }
}

impl std::error::Error for XmlWildcardExcludedNamesError {}

impl SchemaNode {
    pub fn with_xml_wildcard_excluded_names(
        mut self,
        names: XmlWildcardExcludedNames,
    ) -> Option<Self> {
        self.xml_wildcard_excluded_names = Some(names);
        self.xml_wildcard_excluded_names_are_valid().then_some(self)
    }

    /// Excluded names require valid wildcard namespace metadata on the same
    /// node, and every name must be in a namespace that constraint allows.
    pub fn xml_wildcard_excluded_names_are_valid(&self) -> bool {
        let Some(names) = &self.xml_wildcard_excluded_names else {
            return true;
        };
        self.xml_wildcard_namespace_is_valid()
            && self
                .xml_wildcard_namespace
                .as_ref()
                .is_some_and(|constraint| {
                    names
                        .iter()
                        .all(|name| constraint.allows(name.namespace().uri()))
                })
    }

    /// Whether this node's own wildcard metadata uses a negated namespace set
    /// or excluded names.
    pub fn has_xml_wildcard_exclusions(&self) -> bool {
        self.xml_wildcard_excluded_names.is_some()
            || matches!(
                self.xml_wildcard_namespace,
                Some(XmlWildcardNamespaceConstraint::Not { .. })
            )
    }

    /// Whether any node in this schema, including nested JSON predicate
    /// schemas, uses wildcard exclusions.
    pub fn uses_xml_wildcard_exclusions(&self) -> bool {
        let predicate_uses = |schema: Option<&SchemaNode>| {
            schema.is_some_and(SchemaNode::uses_xml_wildcard_exclusions)
        };
        self.has_xml_wildcard_exclusions()
            || self.json_contains.as_ref().is_some_and(|constraints| {
                constraints
                    .as_slice()
                    .iter()
                    .any(|constraint| predicate_uses(constraint.predicate().as_schema()))
            })
            || self
                .json_dependent_schemas
                .as_ref()
                .is_some_and(|constraints| {
                    constraints
                        .as_slice()
                        .iter()
                        .any(|constraint| predicate_uses(constraint.predicate().as_schema()))
                })
            || match &self.kind {
                SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => false,
                SchemaKind::Group {
                    children, dynamic, ..
                } => {
                    children
                        .iter()
                        .any(SchemaNode::uses_xml_wildcard_exclusions)
                        || dynamic
                            .as_deref()
                            .is_some_and(SchemaNode::uses_xml_wildcard_exclusions)
                }
            }
    }
}
