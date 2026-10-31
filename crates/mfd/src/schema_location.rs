//! The reference application's default XML output schema-location hint.
//!
//! An XML target whose `properties/@AddSchemaLocation` is absent or `"1"`
//! annotates its output root with the target schema's location; `"0"` writes
//! nothing. The reference application writes an absolute `file:///` URI for
//! the machine it runs on. Ferrule writes the schema reference the design
//! stores instead, as a URI reference, so the hint stays portable.

use ir::{SchemaNode, XmlNamespace, XmlSchemaHintDerivation, XmlSchemaHints, XmlSchemaLocation};

pub(crate) const PROPERTY: &str = "AddSchemaLocation";
const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";

/// The hint an XML document target writes for its stored setting and schema
/// reference, marked as derived from that reference. `schema_namespace` is the
/// referenced schema's own `targetNamespace` (`None` for a no-namespace
/// schema). The location names that schema, so it describes the root only when
/// the namespaces agree; a root declared in an imported schema gets no hint.
/// `Ok(None)` means no hint; `Err` names a setting the hint policy cannot
/// represent.
pub(crate) fn derived_hints(
    setting: Option<&str>,
    schema_reference: Option<&str>,
    schema_namespace: Option<&str>,
    root: &SchemaNode,
) -> Result<Option<XmlSchemaHints>, String> {
    match setting {
        None | Some("1") => {}
        Some("0") => return Ok(None),
        Some(other) => return Err(format!("unsupported {PROPERTY} value `{other}`")),
    }
    // DTD references and schema-less documents carry no XML Schema hint.
    let Some(reference) = schema_reference.filter(|reference| !is_dtd(reference)) else {
        return Ok(None);
    };
    let namespace = root_namespace(root);
    if schema_namespace != namespace {
        return Ok(None);
    }
    // A root that declares the attribute itself keeps the mapped value.
    if declares_hint_attribute(root, namespace.is_some()) {
        return Ok(None);
    }
    let location = uri_reference(reference);
    let derivation = Some(XmlSchemaHintDerivation {
        schema_reference: reference.to_owned(),
    });
    let hints = match namespace {
        None => XmlSchemaHints {
            no_namespace_location: Some(location),
            locations: Vec::new(),
            derivation,
        },
        Some(namespace) => XmlSchemaHints {
            no_namespace_location: None,
            locations: vec![XmlSchemaLocation {
                namespace: namespace.to_owned(),
                location,
            }],
            derivation,
        },
    };
    hints
        .validate()
        .map_err(|error| format!("schema location `{reference}` is not a valid hint: {error}"))?;
    Ok(Some(hints))
}

/// Whether `hints` are still exactly what the default setting derives from
/// their recorded schema reference for `root`. Native export then writes that
/// setting, and the location follows the exported schema. Hints without a
/// derivation, or whose locations, namespace or root no longer agree with it,
/// are explicit.
pub(crate) fn is_derived(hints: &XmlSchemaHints, root: &SchemaNode) -> bool {
    let Some(derivation) = &hints.derivation else {
        return false;
    };
    // Import already proved the schema's own namespace; the root decides now.
    derived_hints(
        None,
        Some(&derivation.schema_reference),
        root_namespace(root),
        root,
    )
    .is_ok_and(|derived| derived.as_ref() == Some(hints))
}

/// The `targetNamespace` of a schema document: `Some(None)` for a
/// no-namespace schema, `None` when the text is not a readable schema.
pub(crate) fn schema_target_namespace(text: &str) -> Option<Option<String>> {
    let document = roxmltree::Document::parse(text).ok()?;
    let root = document.root_element();
    (root.tag_name().name() == "schema"
        && root.tag_name().namespace() == Some("http://www.w3.org/2001/XMLSchema"))
    .then(|| {
        root.attribute("targetNamespace")
            .filter(|namespace| !namespace.is_empty())
            .map(str::to_owned)
    })
}

fn root_namespace(root: &SchemaNode) -> Option<&str> {
    match &root.xml_namespace {
        Some(XmlNamespace::Qualified(uri)) => Some(uri.as_str()),
        _ => None,
    }
}

fn is_dtd(reference: &str) -> bool {
    std::path::Path::new(&reference.replace('\\', "/"))
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dtd"))
}

fn declares_hint_attribute(root: &SchemaNode, qualified: bool) -> bool {
    let wanted = if qualified {
        "schemaLocation"
    } else {
        "noNamespaceSchemaLocation"
    };
    let ir::SchemaKind::Group { children, .. } = &root.kind else {
        return false;
    };
    children.iter().any(|child| {
        child.attribute
            && match child.name.split_once(':') {
                Some(("xsi", local)) => local == wanted,
                Some(_) => false,
                None => {
                    child.name == wanted
                        && matches!(
                            &child.xml_namespace,
                            Some(XmlNamespace::Qualified(uri)) if uri.as_str() == XSI
                        )
                }
            }
    })
}

/// RFC 3986 `scheme ":"`, excluding one-letter Windows drive prefixes.
fn has_scheme(reference: &str) -> bool {
    let Some((scheme, _)) = reference.split_once(':') else {
        return false;
    };
    scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Converts a stored schema reference to a URI reference. Windows
/// extended-length prefixes are removed, backslashes become path separators,
/// Windows drive and UNC paths become `file:` URIs, and every byte that cannot
/// appear literally is percent-encoded. A value that already names a URI scheme
/// keeps its existing escapes; a `%` that starts none is encoded.
pub(crate) fn uri_reference(stored: &str) -> String {
    // Windows extended-length prefixes are not part of the path.
    let stored = if let Some(unc) = stored
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| stored.strip_prefix("//?/UNC/"))
    {
        format!(r"\\{unc}")
    } else {
        stored
            .strip_prefix(r"\\?\")
            .or_else(|| stored.strip_prefix("//?/"))
            .unwrap_or(stored)
            .to_owned()
    };
    let path = stored.replace('\\', "/");
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/');
    let (prefix, rest, uri) = if has_scheme(&path) {
        ("", path.as_str(), true)
    } else if drive {
        // Keep the drive colon; the rest is an ordinary path.
        ("file:///", path.as_str(), false)
    } else if path.starts_with("//") {
        ("file:", path.as_str(), false)
    } else {
        ("", path.as_str(), false)
    };
    let mut out = String::with_capacity(prefix.len() + rest.len());
    out.push_str(prefix);
    for (index, byte) in rest.bytes().enumerate() {
        let literal = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'@'
                    | b'/'
            )
            || (uri && matches!(byte, b':' | b'?' | b'#' | b'[' | b']'))
            // A URI keeps its existing escapes; a stray `%` is encoded.
            || (uri
                && byte == b'%'
                && rest.as_bytes()[index + 1..]
                    .get(..2)
                    .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit)))
            || (drive && index == 1);
        if literal {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;

    fn root(namespace: Option<&str>) -> SchemaNode {
        let root = SchemaNode::group(
            "Envelope",
            vec![SchemaNode::scalar("Amount", ScalarType::Int)],
        );
        match namespace {
            Some(uri) => root.xml_qualified(uri).unwrap(),
            None => root,
        }
    }

    #[test]
    fn stored_paths_become_portable_uri_references() {
        assert_eq!(uri_reference("target.xsd"), "target.xsd");
        assert_eq!(
            uri_reference(r"schemas\Target Schema.xsd"),
            "schemas/Target%20Schema.xsd"
        );
        assert_eq!(uri_reference("../a%b#c?.xsd"), "../a%25b%23c%3F.xsd");
        assert_eq!(uri_reference("Größe.xsd"), "Gr%C3%B6%C3%9Fe.xsd");
        assert_eq!(uri_reference(r"C:\Data\t.xsd"), "file:///C:/Data/t.xsd");
        assert_eq!(
            uri_reference(r"\\host\share\t.xsd"),
            "file://host/share/t.xsd"
        );
        assert_eq!(
            uri_reference("http://example.com/a%20b.xsd?x#y"),
            "http://example.com/a%20b.xsd?x#y"
        );
        assert_eq!(uri_reference("a:b.xsd"), "a%3Ab.xsd");
    }

    #[test]
    fn extended_length_prefixes_and_stray_percents_convert() {
        assert_eq!(uri_reference(r"\\?\C:\Data\t.xsd"), "file:///C:/Data/t.xsd");
        assert_eq!(uri_reference("//?/C:/Data/t.xsd"), "file:///C:/Data/t.xsd");
        assert_eq!(
            uri_reference(r"\\?\UNC\host\share\t.xsd"),
            "file://host/share/t.xsd"
        );
        assert_eq!(
            uri_reference("http://example.com/100%/a%2Fb%zz%4.xsd"),
            "http://example.com/100%25/a%2Fb%25zz%254.xsd"
        );
        assert_eq!(
            uri_reference("http://example.com/end%"),
            "http://example.com/end%25"
        );
    }

    #[test]
    fn hints_name_only_a_schema_whose_namespace_is_the_roots() {
        let plain = root(None);
        let qualified = root(Some("urn:t"));
        // A root declared in an imported schema is not described by the
        // stored schema, so no pair maps its namespace to that file.
        assert_eq!(
            derived_hints(None, Some("wrapper.xsd"), Some("urn:other"), &qualified),
            Ok(None)
        );
        assert_eq!(
            derived_hints(None, Some("wrapper.xsd"), None, &qualified),
            Ok(None)
        );
        assert_eq!(
            derived_hints(None, Some("wrapper.xsd"), Some("urn:t"), &plain),
            Ok(None)
        );
        assert_eq!(
            schema_target_namespace(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:t"/>"#
            ),
            Some(Some("urn:t".into()))
        );
        assert_eq!(
            schema_target_namespace(r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"/>"#),
            Some(None)
        );
        assert_eq!(schema_target_namespace("<!ELEMENT a (#PCDATA)>"), None);
    }

    #[test]
    fn setting_selects_hint_form_by_root_namespace() {
        let plain = root(None);
        let expected = XmlSchemaHints {
            no_namespace_location: Some("target.xsd".into()),
            locations: Vec::new(),
            derivation: Some(XmlSchemaHintDerivation {
                schema_reference: "target.xsd".into(),
            }),
        };
        for setting in [None, Some("1")] {
            assert_eq!(
                derived_hints(setting, Some("target.xsd"), None, &plain),
                Ok(Some(expected.clone()))
            );
        }
        assert_eq!(
            derived_hints(Some("0"), Some("target.xsd"), None, &plain),
            Ok(None)
        );
        assert_eq!(
            derived_hints(None, Some("target.DTD"), None, &plain),
            Ok(None)
        );
        assert_eq!(derived_hints(None, None, None, &plain), Ok(None));
        assert!(derived_hints(Some("yes"), Some("target.xsd"), None, &plain).is_err());

        let qualified = root(Some("urn:t"));
        assert_eq!(
            derived_hints(None, Some(r"x\t.xsd"), Some("urn:t"), &qualified),
            Ok(Some(XmlSchemaHints {
                no_namespace_location: None,
                locations: vec![XmlSchemaLocation {
                    namespace: "urn:t".into(),
                    location: "x/t.xsd".into(),
                }],
                derivation: Some(XmlSchemaHintDerivation {
                    schema_reference: r"x\t.xsd".into(),
                }),
            }))
        );
        assert!(is_derived(&expected, &plain));
        assert!(!is_derived(&expected, &qualified));
    }

    #[test]
    fn declared_root_attribute_suppresses_the_automatic_hint() {
        let mut declared = root(None);
        let ir::SchemaKind::Group { children, .. } = &mut declared.kind else {
            unreachable!()
        };
        children.push(
            SchemaNode::scalar("noNamespaceSchemaLocation", ScalarType::String)
                .attribute()
                .xml_qualified(XSI)
                .unwrap(),
        );
        assert_eq!(
            derived_hints(None, Some("target.xsd"), None, &declared),
            Ok(None)
        );
    }

    #[test]
    fn only_unedited_derivations_stay_derived() {
        let plain = root(None);
        let derived = derived_hints(None, Some(r"C:\Data\t.xsd"), None, &plain)
            .unwrap()
            .unwrap();
        assert!(is_derived(&derived, &plain));
        // The same locations without provenance are explicit.
        assert!(!is_derived(&derived.without_derivation(), &plain));
        // Editing the location or the recorded reference makes them explicit.
        let mut edited = derived.clone();
        edited.no_namespace_location = Some("other.xsd".into());
        assert!(!is_derived(&edited, &plain));
        let mut moved = derived.clone();
        moved.derivation = Some(XmlSchemaHintDerivation {
            schema_reference: "other.xsd".into(),
        });
        assert!(!is_derived(&moved, &plain));
        let mut paired = derived;
        paired.locations.push(XmlSchemaLocation {
            namespace: "urn:x".into(),
            location: "x.xsd".into(),
        });
        assert!(!is_derived(&paired, &plain));
    }
}
