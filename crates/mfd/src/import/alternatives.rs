use std::collections::BTreeMap;
use std::path::Path;

use ir::{GroupAlternative, SchemaKind, SchemaNode};

pub(super) fn conditioned_port_types(structure: &roxmltree::Node<'_, '_>) -> BTreeMap<u32, String> {
    let mut types = BTreeMap::new();
    for entry in structure
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
    {
        let Some(type_name) = conditioned_type_name(&entry) else {
            continue;
        };
        for key in [entry.attribute("outkey"), entry.attribute("inpkey")]
            .into_iter()
            .flatten()
            .filter_map(|key| key.parse::<u32>().ok())
        {
            types.insert(key, type_name.clone());
        }
    }
    types
}

pub(super) fn merge_conditioned_xml_types(
    entry: &roxmltree::Node,
    schema: &mut SchemaNode,
    xsd_path: &Path,
    warnings: &mut Vec<String>,
) {
    merge_selected_roots(entry, schema, xsd_path, warnings, &mut Vec::new());
    merge_entry_children(entry, schema, xsd_path, warnings, &mut Vec::new());
}

fn merge_selected_roots(
    entry: &roxmltree::Node,
    schema: &mut SchemaNode,
    xsd_path: &Path,
    warnings: &mut Vec<String>,
    path: &mut Vec<String>,
) {
    let entries = entry
        .children()
        .filter(|child| child.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let selected = entries
        .iter()
        .filter(|child| child.attribute("name") == Some("*"))
        .flat_map(|wildcard| wildcard.descendants())
        .filter(|node| node.has_tag_name("qname"))
        .filter_map(|node| node.attribute("QNameAsString"))
        .filter(|qname| !qname.is_empty())
        .collect::<Vec<_>>();

    if let SchemaKind::Group { children, .. } = &mut schema.kind {
        for qname in selected {
            let name = qname.rsplit('}').next().unwrap_or(qname);
            if children.iter().any(|child| child.name == name) {
                continue;
            }
            match format_xml::xsd::import_root(xsd_path, Some(qname)) {
                Ok(mut selected_schema) => {
                    // A concrete QName selected from `xs:any` is still a
                    // sequence projection over wildcard children, even when
                    // the selected document root itself is singular.
                    selected_schema.repeating = true;
                    children.push(selected_schema);
                }
                Err(error) => warnings.push(format!(
                    "selected XML element `{}` could not be resolved from the schema: {error}",
                    display_child_path(path, name)
                )),
            }
        }
    }

    for child_entry in entries {
        let name = normalized_entry_name(child_entry.attribute("name").unwrap_or_default());
        if name == "*" {
            continue;
        }
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            continue;
        };
        let Some(child_schema) = children.iter_mut().find(|child| child.name == name) else {
            continue;
        };
        path.push(name);
        merge_selected_roots(&child_entry, child_schema, xsd_path, warnings, path);
        path.pop();
    }
}

fn display_child_path(path: &[String], child: &str) -> String {
    if path.is_empty() {
        child.to_string()
    } else {
        format!("{}/{child}", path.join("/"))
    }
}

fn merge_entry_children(
    entry: &roxmltree::Node,
    schema: &mut SchemaNode,
    xsd_path: &Path,
    warnings: &mut Vec<String>,
    path: &mut Vec<String>,
) {
    let children: Vec<_> = entry
        .children()
        .filter(|child| child.has_tag_name("entry"))
        .collect();
    let mut conditioned: BTreeMap<String, Vec<roxmltree::Node<'_, '_>>> = BTreeMap::new();
    for child in &children {
        if child.children().any(|node| node.has_tag_name("condition")) {
            conditioned
                .entry(normalized_entry_name(
                    child.attribute("name").unwrap_or_default(),
                ))
                .or_default()
                .push(*child);
        }
    }
    for (name, entries) in conditioned {
        if entries.len() < 2 {
            continue;
        }
        path.push(name);
        if let Err(reason) = merge_alternatives_at(schema, path, &entries, xsd_path) {
            warnings.push(format!(
                "conditional XML type alternatives at `{}` could not be represented: {reason}",
                path.join("/")
            ));
        }
        path.pop();
    }

    for child in children {
        let name = normalized_entry_name(child.attribute("name").unwrap_or_default());
        path.push(name);
        merge_entry_children(&child, schema, xsd_path, warnings, path);
        path.pop();
    }
}

fn merge_alternatives_at(
    schema: &mut SchemaNode,
    path: &[String],
    entries: &[roxmltree::Node<'_, '_>],
    xsd_path: &Path,
) -> Result<(), String> {
    let node = schema_node_at_mut(schema, path)
        .ok_or_else(|| "the base schema path does not exist".to_string())?;
    let mut metadata = Vec::with_capacity(entries.len());
    {
        let SchemaKind::Group { children, .. } = &mut node.kind else {
            return Err("the base schema node is not a group".to_string());
        };
        for entry in entries {
            let type_name = conditioned_type_name(entry).ok_or_else(|| {
                "a condition is not an exact equality between xsi:type and a constant QName"
                    .to_string()
            })?;
            let derived = format_xml::xsd::import_type(xsd_path, &type_name)
                .map_err(|error| error.to_string())?;
            let SchemaKind::Group {
                children: derived_children,
                ..
            } = derived.kind
            else {
                return Err(format!("type `{type_name}` is not a complex type"));
            };
            let mut members = Vec::with_capacity(derived_children.len());
            for child in derived_children {
                members.push(child.name.clone());
                if let Some(existing) = children.iter().find(|existing| existing.name == child.name)
                {
                    if existing != &child {
                        return Err(format!(
                            "field `{}` has incompatible schemas across derived types",
                            child.name
                        ));
                    }
                } else {
                    children.push(child);
                }
            }
            metadata.push(GroupAlternative {
                name: type_name,
                members,
                required: Vec::new(),
            });
        }
    }
    node.set_alternatives(metadata)
        .then_some(())
        .ok_or_else(|| "the derived type alternatives have inconsistent metadata".to_string())
}

fn conditioned_type_name(entry: &roxmltree::Node) -> Option<String> {
    let condition = entry
        .children()
        .find(|node| node.has_tag_name("condition"))?;
    let function = condition
        .children()
        .find(|node| node.has_tag_name("expression"))?
        .children()
        .find(|node| node.has_tag_name("function"))?;
    if function.attribute("name") != Some("equal") || function.attribute("library") != Some("core")
    {
        return None;
    }
    let operands: Vec<_> = function
        .children()
        .filter(|node| node.has_tag_name("expression"))
        .collect();
    let [first, second] = operands.as_slice() else {
        return None;
    };
    qname_equality_operands(first, second).or_else(|| qname_equality_operands(second, first))
}

fn qname_equality_operands(
    attribute_expression: &roxmltree::Node,
    constant_expression: &roxmltree::Node,
) -> Option<String> {
    let attribute = attribute_expression
        .children()
        .find(|node| node.has_tag_name("attribute"))?;
    if attribute.attribute("name") != Some("type")
        || attribute.attribute("ns") != Some("http://www.w3.org/2001/XMLSchema-instance")
    {
        return None;
    }
    let constant = constant_expression
        .children()
        .find(|node| node.has_tag_name("constant"))?;
    if constant.attribute("datatype") != Some("QName") {
        return None;
    }
    constant.attribute("value").map(str::to_string)
}

fn schema_node_at_mut<'a>(
    schema: &'a mut SchemaNode,
    path: &[String],
) -> Option<&'a mut SchemaNode> {
    let mut node = schema;
    for segment in path {
        let SchemaKind::Group { children, .. } = &mut node.kind else {
            return None;
        };
        node = children.iter_mut().find(|child| child.name == *segment)?;
    }
    Some(node)
}

fn normalized_entry_name(name: &str) -> String {
    let name = match name.split_once(':') {
        Some((prefix, local))
            if !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            local
        }
        _ => name,
    };
    name.strip_prefix('@').unwrap_or(name).to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ir::ScalarType;

    use super::*;

    #[test]
    fn wildcard_qname_selections_import_their_concrete_schema_roots() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ferrule_mfd_selected_xml_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let main = dir.join("message.xsd");
        std::fs::write(
            dir.join("payload.xsd"),
            r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                    targetNamespace="urn:ferrule:selected">
                <xs:element name="Chosen"><xs:complexType><xs:sequence>
                    <xs:element name="Count" type="xs:int"/>
                </xs:sequence></xs:complexType></xs:element>
            </xs:schema>"###,
        )
        .unwrap();
        std::fs::write(
            &main,
            r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                    targetNamespace="urn:ferrule:message">
                <xs:import namespace="urn:ferrule:selected" schemaLocation="payload.xsd"/>
                <xs:element name="Envelope"><xs:complexType><xs:sequence>
                    <xs:element name="Body"><xs:complexType><xs:sequence>
                        <xs:any namespace="##other" minOccurs="0" maxOccurs="unbounded"/>
                    </xs:sequence></xs:complexType></xs:element>
                </xs:sequence></xs:complexType></xs:element>
            </xs:schema>"###,
        )
        .unwrap();
        let entry = roxmltree::Document::parse(
            r#"<entry name="Envelope"><entry name="Body">
                <entry name="*"><selections>
                    <qname QNameAsString="{urn:ferrule:selected}Chosen"/>
                </selections></entry>
                <entry name="Chosen"><entry name="Count"/></entry>
            </entry></entry>"#,
        )
        .unwrap();
        let mut schema = format_xml::xsd::import_root(&main, Some("Envelope")).unwrap();
        let mut warnings = Vec::new();

        merge_conditioned_xml_types(&entry.root_element(), &mut schema, &main, &mut warnings);
        std::fs::remove_dir_all(dir).unwrap();

        assert!(warnings.is_empty(), "{warnings:?}");
        let chosen = schema.child("Body").unwrap().child("Chosen").unwrap();
        assert!(chosen.repeating);
        assert!(matches!(
            chosen.child("Count").unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));
    }
}
