//! The declared XML occurrence and nil admission of one schema node.
//!
//! XSD export writes element occurrences, attribute use and `nillable` from
//! this declaration, and the instance reader and writer admit `xsi:nil` from
//! it. It derives from the effective XML view, so JSON-only metadata is never
//! consulted and contradictory XML roles have no declaration.

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
