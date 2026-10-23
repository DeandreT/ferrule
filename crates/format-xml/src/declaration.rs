//! The declared XML occurrence and nil admission of one schema node.
//!
//! XSD export writes element occurrences, attribute use and `nillable` from
//! this declaration, and the instance reader and writer admit `xsi:nil` from
//! it. XSD import compares restrictions, xsi:type member profiles and
//! substitution-group heads through it. It derives from the effective XML
//! view, so JSON-only metadata is never consulted and contradictory XML roles
//! have no declaration.

use ir::SchemaNode;

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
