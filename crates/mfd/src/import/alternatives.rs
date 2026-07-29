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
    schema_from_entry_tree: bool,
    warnings: &mut Vec<String>,
) {
    merge_selected_roots(
        entry,
        schema,
        xsd_path,
        schema_from_entry_tree,
        warnings,
        &mut Vec::new(),
    );
    merge_entry_children(
        entry,
        schema,
        xsd_path,
        schema_from_entry_tree,
        warnings,
        &mut Vec::new(),
    );
}

fn merge_selected_roots(
    entry: &roxmltree::Node,
    schema: &mut SchemaNode,
    xsd_path: &Path,
    schema_from_entry_tree: bool,
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
        .fold(
            BTreeMap::<String, Vec<&str>>::new(),
            |mut selected, qname| {
                let name = qname.rsplit('}').next().unwrap_or(qname).to_string();
                let alternatives = selected.entry(name).or_default();
                if !alternatives.contains(&qname) {
                    alternatives.push(qname);
                }
                selected
            },
        );

    if let SchemaKind::Group { children, .. } = &mut schema.kind {
        for (name, qnames) in selected {
            let [qname] = qnames.as_slice() else {
                warnings.push(format!(
                    "selected XML element `{}` could not be resolved because multiple qualified names share that mapping path: {}",
                    display_child_path(path, &name),
                    qnames.join(", ")
                ));
                continue;
            };
            match format_xml::xsd::import_root(xsd_path, Some(qname)) {
                Ok(mut selected_schema) => {
                    // A concrete QName selected from `xs:any` is still a
                    // sequence projection over wildcard children, even when
                    // the selected document root itself is singular.
                    selected_schema.repeating = true;
                    match children.iter().position(|child| child.name == name) {
                        Some(index) if schema_from_entry_tree => {
                            if let Err(reason) =
                                fallback_entry_shape_fits(&children[index], &selected_schema)
                            {
                                warnings.push(format!(
                                    "selected XML element `{}` could not refine the fallback entry tree exactly: {reason}; fallback retained",
                                    display_child_path(path, &name)
                                ));
                            } else {
                                children[index] = selected_schema;
                            }
                        }
                        Some(_) => {
                            let mismatch = entries
                                .iter()
                                .filter(|entry| {
                                    normalized_entry_name(
                                        entry.attribute("name").unwrap_or_default(),
                                    ) == name
                                })
                                .find_map(|entry| {
                                    let exposed = super::schema::entry_tree_schema(entry);
                                    fallback_entry_shape_fits(&exposed, &selected_schema).err()
                                });
                            if let Some(reason) = mismatch {
                                warnings.push(format!(
                                    "selected XML element `{}` is incompatible with its exposed mapping ports: {reason}; the resolved schema was retained and incompatible scalar connections are non-executable",
                                    display_child_path(path, &name)
                                ));
                            }
                        }
                        None => children.push(selected_schema),
                    }
                }
                Err(error) => warnings.push(format!(
                    "selected XML element `{}` could not be resolved from the schema: {error}",
                    display_child_path(path, &name)
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
        merge_selected_roots(
            &child_entry,
            child_schema,
            xsd_path,
            schema_from_entry_tree,
            warnings,
            path,
        );
        path.pop();
    }
}

fn fallback_entry_shape_fits(fallback: &SchemaNode, selected: &SchemaNode) -> Result<(), String> {
    fallback_entry_shape_fits_inner(fallback, selected, true)
}

fn fallback_entry_shape_fits_inner(
    fallback: &SchemaNode,
    selected: &SchemaNode,
    root: bool,
) -> Result<(), String> {
    let SchemaKind::Group {
        children: fallback_children,
        ..
    } = &fallback.kind
    else {
        return if root || selected.is_scalar() {
            Ok(())
        } else {
            Err(format!(
                "field `{}` is exposed as a scalar but the selected declaration is structured",
                fallback.name
            ))
        };
    };
    let SchemaKind::Group {
        children: selected_children,
        ..
    } = &selected.kind
    else {
        return Err(format!(
            "entry `{}` exposes child fields but the selected declaration is scalar",
            fallback.name
        ));
    };
    for fallback_child in fallback_children {
        let Some(selected_child) = selected_children
            .iter()
            .find(|selected_child| selected_child.name == fallback_child.name)
        else {
            return Err(format!(
                "field `{}` is not declared by the selected schema",
                fallback_child.name
            ));
        };
        fallback_entry_shape_fits_inner(fallback_child, selected_child, false)?;
    }
    Ok(())
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
    schema_from_entry_tree: bool,
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
        path.push(name);
        if let Err(reason) =
            merge_alternatives_at(schema, path, &entries, xsd_path, schema_from_entry_tree)
        {
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
        merge_entry_children(
            &child,
            schema,
            xsd_path,
            schema_from_entry_tree,
            warnings,
            path,
        );
        path.pop();
    }
}

fn merge_alternatives_at(
    schema: &mut SchemaNode,
    path: &[String],
    entries: &[roxmltree::Node<'_, '_>],
    xsd_path: &Path,
    schema_from_entry_tree: bool,
) -> Result<(), String> {
    let node = schema_node_at_mut(schema, path)
        .ok_or_else(|| "the base schema path does not exist".to_string())?;
    let mut imported = Vec::with_capacity(entries.len());
    for entry in entries {
        let type_name = conditioned_type_name(entry).ok_or_else(|| {
            "a condition is not an exact equality between xsi:type and a constant QName".to_string()
        })?;
        let base_name = (entries.len() == 1 || schema_from_entry_tree)
            .then(|| format_xml::xsd::import_type_base(xsd_path, &type_name))
            .transpose()
            .map_err(|error| error.to_string())?
            .flatten();
        let derived = format_xml::xsd::import_type(xsd_path, &type_name)
            .map_err(|error| error.to_string())?;
        let SchemaKind::Group {
            children: derived_children,
            ..
        } = derived.kind
        else {
            return Err(format!("type `{type_name}` is not a complex type"));
        };
        imported.push((type_name, base_name, derived_children));
    }

    let mut metadata = node.alternatives().to_vec();
    metadata.reserve(entries.len() + usize::from(entries.len() == 1));
    let promote_fallback = schema_from_entry_tree
        && matches!(
            node.kind,
            SchemaKind::Scalar {
                ty: ir::ScalarType::String
            }
        )
        && !node.attribute
        && !node.text
        && !node.repeating;
    if promote_fallback {
        let common_base = imported
            .first()
            .and_then(|(_, base, _)| base.as_ref())
            .filter(|base| {
                imported
                    .iter()
                    .all(|(_, candidate, _)| candidate.as_ref() == Some(*base))
            })
            .cloned();
        let base_children = common_base
            .as_ref()
            .map(|base| {
                format_xml::xsd::import_type(xsd_path, base)
                    .map_err(|error| error.to_string())
                    .and_then(|base_schema| match base_schema.kind {
                        SchemaKind::Group { children, .. } => Ok(children),
                        _ => Err(format!("base type `{base}` is not a complex type")),
                    })
            })
            .transpose()?;
        let children = base_children.unwrap_or_default();
        if let Some(base) = common_base {
            metadata.push(GroupAlternative {
                name: base,
                members: children.iter().map(|child| child.name.clone()).collect(),
                required: Vec::new(),
                constraints: Vec::new(),
            });
        }
        node.kind = SchemaKind::Group {
            children,
            dynamic: None,
            alternatives: Vec::new(),
            xml_restricted_alternatives: Vec::new(),
        };
    }
    {
        let SchemaKind::Group { children, .. } = &mut node.kind else {
            return Err("the base schema node is not a group".to_string());
        };
        if metadata.is_empty()
            && let [(.., Some(base_name), _)] = imported.as_slice()
        {
            metadata.push(GroupAlternative {
                name: base_name.clone(),
                members: children.iter().map(|child| child.name.clone()).collect(),
                required: Vec::new(),
                constraints: Vec::new(),
            });
        }
        for (type_name, _, derived_children) in imported {
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
            match metadata
                .iter()
                .find(|alternative| alternative.name == type_name)
            {
                Some(alternative) if alternative.members != members => {
                    return Err(format!(
                        "type `{type_name}` has incompatible members in the imported schema"
                    ));
                }
                Some(_) => {}
                None => metadata.push(GroupAlternative {
                    name: type_name,
                    members,
                    required: Vec::new(),
                    constraints: Vec::new(),
                }),
            }
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
        let mut schema = SchemaNode::group("Envelope", vec![SchemaNode::group("Body", Vec::new())]);
        let mut warnings = Vec::new();

        merge_conditioned_xml_types(
            &entry.root_element(),
            &mut schema,
            &main,
            false,
            &mut warnings,
        );
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

    #[test]
    fn conditioned_complex_types_promote_only_fallback_string_leaves()
    -> Result<(), Box<dyn std::error::Error>> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ferrule_mfd_fallback_alternatives_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let schema_path = dir.join("types.xsd");
        std::fs::write(
            &schema_path,
            r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
                    targetNamespace="urn:ferrule:fallback-types"
                    xmlns:t="urn:ferrule:fallback-types">
                <xs:complexType name="Address">
                    <xs:sequence>
                        <xs:element name="Name" type="xs:string"/>
                    </xs:sequence>
                </xs:complexType>
                <xs:complexType name="Domestic">
                    <xs:complexContent><xs:extension base="t:Address"><xs:sequence>
                        <xs:element name="State" type="xs:string"/>
                    </xs:sequence></xs:extension></xs:complexContent>
                </xs:complexType>
                <xs:complexType name="International">
                    <xs:complexContent><xs:extension base="t:Address"><xs:sequence>
                        <xs:element name="Country" type="xs:string"/>
                    </xs:sequence></xs:extension></xs:complexContent>
                </xs:complexType>
            </xs:schema>"#,
        )?;
        let entry = roxmltree::Document::parse(
            r#"<entry name="Root">
                <entry name="Address">
                    <condition><expression><function name="equal" library="core">
                        <expression><attribute name="type" ns="http://www.w3.org/2001/XMLSchema-instance"/></expression>
                        <expression><constant datatype="QName" value="{urn:ferrule:fallback-types}Domestic"/></expression>
                    </function></expression></condition>
                </entry>
                <entry name="Address" clone="1">
                    <condition><expression><function name="equal" library="core">
                        <expression><attribute name="type" ns="http://www.w3.org/2001/XMLSchema-instance"/></expression>
                        <expression><constant datatype="QName" value="{urn:ferrule:fallback-types}International"/></expression>
                    </function></expression></condition>
                </entry>
            </entry>"#,
        )?;
        let fallback = SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar("Address", ScalarType::String)],
        );

        let mut schema_backed = fallback.clone();
        let mut schema_warnings = Vec::new();
        merge_conditioned_xml_types(
            &entry.root_element(),
            &mut schema_backed,
            &schema_path,
            false,
            &mut schema_warnings,
        );
        assert_eq!(schema_warnings.len(), 1);
        assert!(
            schema_backed
                .child("Address")
                .is_some_and(SchemaNode::is_scalar)
        );

        let mut entry_backed = fallback;
        let mut fallback_warnings = Vec::new();
        merge_conditioned_xml_types(
            &entry.root_element(),
            &mut entry_backed,
            &schema_path,
            true,
            &mut fallback_warnings,
        );
        std::fs::remove_dir_all(dir)?;

        assert!(fallback_warnings.is_empty(), "{fallback_warnings:?}");
        let address = entry_backed
            .child("Address")
            .ok_or("fallback address was not retained")?;
        assert_eq!(address.alternatives().len(), 3);
        assert_eq!(
            address.alternatives()[0].name,
            "{urn:ferrule:fallback-types}Address"
        );
        assert!(address.child("Name").is_some());
        assert!(address.child("State").is_some());
        assert!(address.child("Country").is_some());
        Ok(())
    }
}
