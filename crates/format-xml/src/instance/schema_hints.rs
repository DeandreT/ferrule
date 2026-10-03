use std::collections::{BTreeMap, BTreeSet};

use ir::XmlSchemaHints;
use quick_xml::events::BytesStart;

use super::{XmlFormatError, push_attribute};

const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS: &str = "http://www.w3.org/2000/xmlns/";
const MAX_ROOT_HEADER_BYTES: usize = 1024 * 1024;

fn collision(reason: impl Into<String>) -> XmlFormatError {
    XmlFormatError::SchemaHintCollision(reason.into())
}

/// Runs only for an explicitly decorated root, before its start event is emitted.
/// The inspected allocation is bounded by the root header, never the document.
pub(super) fn push_root_hints(
    start: &mut BytesStart<'_>,
    hints: Option<&XmlSchemaHints>,
) -> Result<(), XmlFormatError> {
    let Some(hints) = hints else {
        return Ok(());
    };
    hints
        .validate()
        .map_err(XmlFormatError::InvalidSchemaHints)?;
    if start.len() > MAX_ROOT_HEADER_BYTES {
        return Err(collision("root header exceeds 1 MiB"));
    }
    let decoder = quick_xml::Reader::from_str("").decoder();
    let mut namespaces = BTreeMap::new();
    namespaces.insert("xml".to_owned(), XML.to_owned());
    let mut attributes = Vec::new();
    let mut declaration_names = BTreeSet::new();
    for attribute in start.attributes().with_checks(false) {
        let attribute = attribute.map_err(|e| collision(e.to_string()))?;
        let name = std::str::from_utf8(attribute.key.as_ref())
            .map_err(|e| collision(e.to_string()))?
            .to_owned();
        let value = attribute
            .decoded_and_normalized_value(quick_xml::XmlVersion::Implicit1_0, decoder)
            .map_err(|e| collision(e.to_string()))?
            .into_owned();
        if let Some(prefix) = name.strip_prefix("xmlns:") {
            if !declaration_names.insert(prefix.to_owned()) {
                return Err(collision("duplicate namespace declaration"));
            }
            if prefix.is_empty()
                || prefix == "xmlns"
                || value == XMLNS
                || (prefix == "xml" && value != XML)
                || (prefix != "xml" && value == XML)
                || (prefix == "xsi" && value != XSI)
                || value.is_empty()
            {
                return Err(collision("reserved or conflicting namespace declaration"));
            }
            namespaces.insert(prefix.to_owned(), value);
        } else if name == "xmlns" {
            if !declaration_names.insert(String::new()) || value == XML || value == XMLNS {
                return Err(collision(
                    "duplicate or reserved default namespace declaration",
                ));
            }
        } else {
            attributes.push(name);
        }
    }
    let mut expanded = BTreeSet::<(&str, &str)>::new();
    for name in &attributes {
        let (namespace, local) = match name.split_once(':') {
            Some((prefix, local))
                if !prefix.is_empty() && !local.is_empty() && !local.contains(':') =>
            {
                (
                    namespaces
                        .get(prefix)
                        .ok_or_else(|| collision("unbound attribute prefix"))?
                        .as_str(),
                    local,
                )
            }
            Some(_) => return Err(collision("malformed attribute QName")),
            None => ("", name.as_str()),
        };
        if !expanded.insert((namespace, local)) {
            return Err(collision("duplicate expanded root attribute"));
        }
        if namespace == XSI
            && ((local == "noNamespaceSchemaLocation" && hints.no_namespace_location.is_some())
                || (local == "schemaLocation" && !hints.locations.is_empty()))
        {
            return Err(collision(
                "mapped attribute already supplies the configured schema hint",
            ));
        }
    }
    if !namespaces.contains_key("xsi") {
        start.push_attribute(("xmlns:xsi", XSI));
    }
    if let Some(location) = &hints.no_namespace_location {
        push_attribute(start, "xsi:noNamespaceSchemaLocation", location);
    }
    if !hints.locations.is_empty() {
        let value = hints
            .locations
            .iter()
            .flat_map(|pair| [pair.namespace.as_str(), pair.location.as_str()])
            .collect::<Vec<_>>()
            .join(" ");
        push_attribute(start, "xsi:schemaLocation", &value);
    }
    Ok(())
}
