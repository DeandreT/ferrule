use ir::{ScalarType, SchemaNode};

use crate::MfdError;

const ANNOTATION: &str = "ferrule-xml-attribute-required";

/// Reject malformed private metadata before an authoritative schema or an
/// untyped fallback can hide it. Explicit zero still needs an attribute role.
pub(in crate::import) fn validate_xml_attribute_required_metadata(
    mapping: &roxmltree::Node<'_, '_>,
) -> Result<(), MfdError> {
    for entry in mapping
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
    {
        decode(&entry)?;
    }
    Ok(())
}

pub(super) fn decode(entry: &roxmltree::Node<'_, '_>) -> Result<Option<bool>, MfdError> {
    let Some(value) = entry.attribute(ANNOTATION) else {
        return Ok(None);
    };
    let fail = |reason| MfdError::InvalidXmlAttributeRequiredMetadata {
        entry: entry.attribute("name").unwrap_or_default().to_string(),
        value: value.to_string(),
        reason,
    };
    let required = match value {
        "0" => false,
        "1" => true,
        _ => return Err(fail("expected exactly `0` or `1`")),
    };
    let component = entry
        .ancestors()
        .find(|node| node.has_tag_name("component"));
    if !component.is_some_and(|component| {
        matches!(
            (component.attribute("library"), component.attribute("kind")),
            (Some("xml"), Some("14")) | (Some("wsdl"), Some("17"))
        )
    }) {
        return Err(fail("requires an XML or WSDL schema component"));
    }
    // Entry-tree decoding follows only direct entry children. A metadata
    // wrapper must not make an annotated declaration silently unreachable.
    if !entry
        .ancestors()
        .skip(1)
        .find(|node| !node.has_tag_name("entry"))
        .is_some_and(|root| {
            root.has_tag_name("root")
                && root
                    .parent()
                    .is_some_and(|data| data.has_tag_name("data") && data.parent() == component)
        })
    {
        return Err(fail("requires an entry inside its owning schema data root"));
    }
    let (name, legacy_attribute) =
        super::normalize_xml_entry_name(entry.attribute("name").unwrap_or_default());
    let mut role = SchemaNode::scalar(name, ScalarType::String);
    role.attribute = legacy_attribute || entry.attribute("type") == Some("attribute");
    // Validate the role even when the transport value is false.
    role.xml_attribute_required = true;
    role.text = entry.attribute("ferrule-text") == Some("1");
    role.repeating = entry.attribute("ferrule-repeating") == Some("1");
    if !role.xml_attribute_required_is_valid()
        || !matches!(entry.attribute("ferrule-kind"), None | Some("scalar"))
        || !matches!(entry.attribute("ferrule-repeating"), None | Some("0"))
        || !matches!(entry.attribute("ferrule-text"), None | Some("0"))
        || !matches!(entry.attribute("type"), None | Some("attribute"))
        || entry
            .descendants()
            .skip(1)
            .any(|node| node.has_tag_name("entry"))
    {
        return Err(fail(
            "requires an ordinary nonrepeating scalar attribute entry",
        ));
    }
    Ok(Some(required))
}
