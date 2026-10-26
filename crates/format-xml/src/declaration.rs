//! Shared XML schema metadata, namespace, occurrence and nil admission.
//!
//! XSD export writes element occurrences, attribute use and `nillable` from
//! this declaration, and the instance reader and writer admit `xsi:nil` from
//! it. XSD import compares restrictions, xsi:type member profiles and
//! substitution-group heads through it. It derives from the effective XML
//! view, so JSON-only metadata is never consulted and contradictory XML roles
//! have no declaration.

use ir::{SchemaKind, SchemaNode, XmlNamespace};

use crate::XmlFormatError;

/// The declared XML occurrence and nil admission of one node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct XmlDeclaration {
    pub(crate) repeated: bool,
    /// One occurrence may be absent: `minOccurs="0"` or an optional attribute.
    pub(crate) optional: bool,
    /// The element occurrence may carry `xsi:nil="true"`.
    pub(crate) nil: bool,
}

/// Derives the declaration from the effective XML view; contradictory XML
/// roles are refused with their typed metadata errors.
pub(crate) fn declaration(node: &SchemaNode) -> Result<XmlDeclaration, XmlFormatError> {
    let view = node.effective_xml_view().map_err(|error| match error {
        ir::EffectiveViewError::XmlAttributeRequiredOutsideAttribute => {
            XmlFormatError::InvalidXmlAttributeRequired {
                name: node.name.clone(),
            }
        }
        _ => XmlFormatError::InvalidXmlOptional {
            name: node.name.clone(),
        },
    })?;
    let (repeated, optional) = match view.occurrence() {
        ir::XmlOccurrence::Single { presence } => {
            (false, presence == ir::EffectivePresence::Optional)
        }
        ir::XmlOccurrence::Repeated => (true, false),
    };
    Ok(XmlDeclaration {
        repeated,
        optional,
        nil: view.nil(),
    })
}

/// `node` is one optional singular element occurrence. Attributes and simple
/// text content have no particle occurrence.
pub(crate) fn optional_element(node: &SchemaNode) -> Result<bool, XmlFormatError> {
    let declared = declaration(node)?;
    Ok(!node.attribute && !declared.repeated && declared.optional)
}

/// `node` is one singular attribute whose use is required.
pub(crate) fn required_attribute(node: &SchemaNode) -> Result<bool, XmlFormatError> {
    let declared = declaration(node)?;
    Ok(node.attribute && !declared.repeated && !declared.optional)
}

/// `restricted` declares a wider occurrence than `base`: it repeats where the
/// base is singular, is an optional element where the base element is not, is
/// an optional attribute where the base attribute is required, or admits
/// `xsi:nil` where the base does not.
pub(crate) fn restriction_widens(
    base: &SchemaNode,
    restricted: &SchemaNode,
) -> Result<bool, XmlFormatError> {
    let (base_declared, restricted_declared) = (declaration(base)?, declaration(restricted)?);
    Ok((!base_declared.repeated && restricted_declared.repeated)
        || (optional_element(restricted)? && !optional_element(base)?)
        || (required_attribute(base)? && !required_attribute(restricted)?)
        || (restricted_declared.nil && !base_declared.nil))
}

/// Shared schema-metadata admission for compilers and instance codecs.
/// Preserve traversal and first-error order independently of either caller.
pub(crate) fn validate_schema(schema: &SchemaNode) -> Result<(), XmlFormatError> {
    if schema.json_array_wrapper {
        return Err(XmlFormatError::UnsupportedSchemaRole {
            node: schema.name.clone(),
            role: "element",
            kind: "JSON array wrapper",
        });
    }
    if !schema.xml_optional_is_valid() {
        return Err(XmlFormatError::InvalidXmlOptional {
            name: schema.name.clone(),
        });
    }
    if !schema.xml_default_type_is_valid() {
        return Err(XmlFormatError::InvalidXmlDefaultType {
            name: schema.name.clone(),
            value: schema.xml_default_type.clone().unwrap_or_default(),
        });
    }
    if !schema.xml_attribute_required_is_valid() {
        return Err(XmlFormatError::InvalidXmlAttributeRequired {
            name: schema.name.clone(),
        });
    }
    validate_schema_default(schema)?;
    if !schema.xml_name_alternatives_are_valid() {
        return Err(XmlFormatError::InvalidXmlNameAlternatives {
            node: schema.name.clone(),
        });
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Ok(());
    };
    for (index, child) in children.iter().enumerate() {
        if let Some(other) = children[..index].iter().find(|other| {
            other.name == child.name
                && (other.xml_namespace != child.xml_namespace
                    || other.xml_name_alternatives != child.xml_name_alternatives)
        }) {
            return Err(XmlFormatError::AmbiguousNamespaceSiblings {
                group: schema.name.clone(),
                name: child.name.clone(),
                first_namespace: namespace_label(other.xml_namespace.as_ref()),
                second_namespace: namespace_label(child.xml_namespace.as_ref()),
            });
        }
        validate_schema(child)?;
    }
    Ok(())
}

fn validate_schema_default(schema: &SchemaNode) -> Result<(), XmlFormatError> {
    if schema.default.is_some() {
        let reason = if schema.fixed.is_some() {
            Some("fixed and default value constraints are mutually exclusive")
        } else if schema.value_generation.is_some() {
            Some("generated and default values are mutually exclusive")
        } else if schema.repeating {
            Some("repeating elements cannot carry a default value")
        } else if !matches!(schema.kind, SchemaKind::Scalar { .. }) {
            Some("only scalar elements, simple content, and attributes support defaults")
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(XmlFormatError::UnsupportedSchemaDefault {
                name: schema.name.clone(),
                reason,
            });
        }
    }
    if schema.repeating
        && schema
            .text_child()
            .is_some_and(|text| text.default.is_some())
    {
        return Err(XmlFormatError::UnsupportedSchemaDefault {
            name: schema.name.clone(),
            reason: "repeating simple-content elements cannot carry a default value",
        });
    }
    Ok(())
}

fn namespace_label(namespace: Option<&XmlNamespace>) -> String {
    match namespace {
        None => "legacy/unspecified".to_string(),
        Some(XmlNamespace::Unqualified) => "unqualified".to_string(),
        Some(XmlNamespace::Qualified(namespace)) => namespace.as_str().to_string(),
    }
}
