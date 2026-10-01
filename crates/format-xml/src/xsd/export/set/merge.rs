use std::collections::{BTreeMap, BTreeSet};

use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use roxmltree::{Document, Node};

use super::{XsdExportArtifact, alternatives};
use crate::XmlFormatError;

const XSD_NAMESPACE: &str = "http://www.w3.org/2001/XMLSchema";

/// An import identifies a namespace, not a list of declaration files. Keep all
/// declarations for that namespace in the first planned artifact so processors
/// that load only the first import retain the complete boundary.
pub(super) fn consolidate(
    root: String,
    dependencies: Vec<XsdExportArtifact>,
) -> Result<(String, Vec<XsdExportArtifact>), XmlFormatError> {
    let mut groups: Vec<(String, Vec<XsdExportArtifact>)> = Vec::new();
    let mut filenames = BTreeMap::new();
    for dependency in dependencies {
        let document = Document::parse(&dependency.contents)?;
        let namespace = document
            .root_element()
            .attribute("targetNamespace")
            .unwrap_or_default();
        let index = groups
            .iter()
            .position(|(existing, _)| existing == namespace)
            .unwrap_or_else(|| {
                groups.push((namespace.to_string(), Vec::new()));
                groups.len() - 1
            });
        let group = &mut groups[index].1;
        let filename = group.first().map_or_else(
            || dependency.filename.clone(),
            |first| first.filename.clone(),
        );
        filenames.insert(dependency.filename.clone(), filename);
        group.push(dependency);
    }
    let root = rewrite_imports(&root, &filenames)?;
    let mut consolidated = Vec::with_capacity(groups.len());
    for (namespace, documents) in groups {
        let first = &documents[0];
        let contents = if documents.len() == 1 {
            rewrite_imports(&first.contents, &filenames)?
        } else {
            merge_namespace(&namespace, &documents, &filenames)?
        };
        consolidated.push(XsdExportArtifact {
            filename: first.filename.clone(),
            contents,
        });
    }
    Ok((root, consolidated))
}

fn rewrite_imports(
    contents: &str,
    filenames: &BTreeMap<String, String>,
) -> Result<String, XmlFormatError> {
    let document = Document::parse(contents)?;
    let mut namespaces = BTreeSet::new();
    let mut out = String::with_capacity(contents.len());
    let mut offset = 0;
    for import in document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name((XSD_NAMESPACE, "import")))
    {
        let range = import.range();
        out.push_str(&contents[offset..range.start]);
        if namespaces.insert(import.attribute("namespace").unwrap_or_default()) {
            let original = &contents[range.clone()];
            let rewritten = import
                .attribute("schemaLocation")
                .and_then(|location| filenames.get(location).map(|next| (location, next)))
                .map_or_else(
                    || original.to_string(),
                    |(location, next)| {
                        original.replace(
                            &format!("schemaLocation=\"{}\"", alternatives::xml_escape(location)),
                            &format!("schemaLocation=\"{}\"", alternatives::xml_escape(next)),
                        )
                    },
                );
            out.push_str(&rewritten);
        }
        offset = range.end;
    }
    out.push_str(&contents[offset..]);
    Ok(out)
}

fn merge_namespace(
    namespace: &str,
    artifacts: &[XsdExportArtifact],
    filenames: &BTreeMap<String, String>,
) -> Result<String, XmlFormatError> {
    let documents = artifacts
        .iter()
        .map(|artifact| Document::parse(&artifact.contents))
        .collect::<Result<Vec<_>, _>>()?;
    let mut prefixes = BTreeMap::<String, String>::new();
    prefixes.insert(XSD_NAMESPACE.to_string(), "xs".to_string());
    prefixes
        .entry(namespace.to_string())
        .or_insert_with(|| "tns".to_string());
    for document in &documents {
        for declaration in document.root_element().namespaces() {
            if prefixes.contains_key(declaration.uri()) {
                continue;
            }
            let preferred = declaration.name().unwrap_or("ns");
            let prefix = if !prefixes.values().any(|existing| existing == preferred) {
                preferred.to_string()
            } else {
                (1..)
                    .map(|index| format!("ns{index}"))
                    .find(|candidate| !prefixes.values().any(|existing| existing == candidate))
                    .unwrap_or_default()
            };
            prefixes.insert(declaration.uri().to_string(), prefix);
        }
    }
    let mut writer = Writer::new(Vec::new());
    let mut schema = BytesStart::new("xs:schema");
    for attribute in documents[0].root_element().attributes() {
        schema.push_attribute((attribute.name(), attribute.value()));
    }
    for (uri, prefix) in &prefixes {
        schema.push_attribute((format!("xmlns:{prefix}").as_str(), uri.as_str()));
    }
    writer.write_event(Event::Start(schema))?;
    let mut imports = BTreeSet::new();
    let mut ordered_imports = Vec::new();
    let mut components = BTreeMap::new();
    let mut ordered_components = Vec::new();
    for document in &documents {
        for node in document.root_element().children().filter(Node::is_element) {
            if node.has_tag_name((XSD_NAMESPACE, "import")) {
                let imported = node.attribute("namespace").unwrap_or_default();
                if imported == namespace || !imports.insert(imported) {
                    continue;
                }
                ordered_imports.push((imported, node.attribute("schemaLocation")));
                continue;
            }
            let component = render(node, &prefixes)?;
            if let Some(name) = node.attribute("name") {
                let role = match node.tag_name().name() {
                    "complexType" | "simpleType" => "type",
                    "element" => "element",
                    "attribute" => "attribute",
                    _ => "declaration",
                };
                let key = (role, name);
                if let Some(existing) = components.get(&key) {
                    if existing != &component {
                        return Err(XmlFormatError::ConflictingNamespaceDeclaration {
                            role,
                            namespace: namespace.to_string(),
                            name: name.to_string(),
                        });
                    }
                    continue;
                }
                components.insert(key, component.clone());
            }
            ordered_components.push(component);
        }
    }
    for (namespace, location) in ordered_imports {
        let mut import = BytesStart::new("xs:import");
        import.push_attribute(("namespace", namespace));
        if let Some(location) = location {
            import.push_attribute((
                "schemaLocation",
                filenames.get(location).map_or(location, String::as_str),
            ));
        }
        writer.write_event(Event::Empty(import))?;
    }
    for component in ordered_components {
        writer.get_mut().extend_from_slice(&component);
    }
    writer.write_event(Event::End(BytesEnd::new("xs:schema")))?;
    Ok(String::from_utf8_lossy(&writer.into_inner()).into_owned())
}

fn render(
    node: Node<'_, '_>,
    prefixes: &BTreeMap<String, String>,
) -> Result<Vec<u8>, XmlFormatError> {
    let mut writer = Writer::new(Vec::new());
    write_node(node, prefixes, &mut writer)?;
    Ok(writer.into_inner())
}

fn write_node(
    node: Node<'_, '_>,
    prefixes: &BTreeMap<String, String>,
    writer: &mut Writer<Vec<u8>>,
) -> Result<(), XmlFormatError> {
    if let Some(text) = node.text().filter(|_| node.is_text()) {
        if !text.trim().is_empty() {
            writer.write_event(Event::Text(BytesText::new(text)))?;
        }
        return Ok(());
    }
    if !node.is_element() {
        return Ok(());
    }
    let name = qualified(
        node.tag_name().namespace(),
        node.tag_name().name(),
        prefixes,
    );
    let mut start = BytesStart::new(&name);
    for attribute in node.attributes() {
        let attribute_name = qualified(attribute.namespace(), attribute.name(), prefixes);
        let value = if attribute.namespace().is_none()
            && node.tag_name().namespace() == Some(XSD_NAMESPACE)
            && matches!(
                attribute.name(),
                "type" | "ref" | "base" | "substitutionGroup"
            ) {
            let (prefix, local) = attribute
                .value()
                .split_once(':')
                .map_or((None, attribute.value()), |(prefix, local)| {
                    (Some(prefix), local)
                });
            let namespace = node.lookup_namespace_uri(prefix);
            if prefix.is_some() && namespace.is_none() {
                return Err(XmlFormatError::UnsupportedNamespaceExport {
                    node: node.tag_name().name().to_string(),
                    namespace: prefix.unwrap_or_default().to_string(),
                    target_namespace: String::new(),
                });
            }
            qualified(namespace, local, prefixes)
        } else {
            attribute.value().to_string()
        };
        start.push_attribute((attribute_name.as_str(), value.as_str()));
    }
    if node.children().any(|child| {
        child.is_element()
            || child.is_text() && child.text().is_some_and(|text| !text.trim().is_empty())
    }) {
        writer.write_event(Event::Start(start))?;
        for child in node.children() {
            write_node(child, prefixes, writer)?;
        }
        writer.write_event(Event::End(BytesEnd::new(&name)))?;
    } else {
        writer.write_event(Event::Empty(start))?;
    }
    Ok(())
}

fn qualified(namespace: Option<&str>, local: &str, prefixes: &BTreeMap<String, String>) -> String {
    namespace
        .and_then(|namespace| prefixes.get(namespace))
        .map_or_else(|| local.to_string(), |prefix| format!("{prefix}:{local}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(filename: &str, declarations: &str) -> XsdExportArtifact {
        XsdExportArtifact {
            filename: filename.to_string(),
            contents: format!(
                r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:tns="urn:ferrule:merge" targetNamespace="urn:ferrule:merge">{declarations}</xs:schema>"#
            ),
        }
    }

    #[test]
    fn conflicting_global_type_definitions_reject() {
        let documents = [
            artifact(
                "first.xsd",
                r#"<xs:simpleType name="Shared"><xs:restriction base="xs:string"/></xs:simpleType>"#,
            ),
            artifact(
                "second.xsd",
                r#"<xs:simpleType name="Shared"><xs:restriction base="xs:integer"/></xs:simpleType>"#,
            ),
        ];
        assert!(matches!(
            merge_namespace("urn:ferrule:merge", &documents, &BTreeMap::new()),
            Err(XmlFormatError::ConflictingNamespaceDeclaration {
                role: "type",
                namespace,
                name,
            }) if namespace == "urn:ferrule:merge" && name == "Shared"
        ));
    }

    #[test]
    fn unresolved_qname_prefixes_reject() {
        let documents = [
            artifact(
                "first.xsd",
                r#"<xs:element name="First" type="missing:Type"/>"#,
            ),
            artifact(
                "second.xsd",
                r#"<xs:element name="Second" type="xs:string"/>"#,
            ),
        ];
        assert!(matches!(
            merge_namespace("urn:ferrule:merge", &documents, &BTreeMap::new()),
            Err(XmlFormatError::UnsupportedNamespaceExport { namespace, .. }) if namespace == "missing"
        ));
    }
}
