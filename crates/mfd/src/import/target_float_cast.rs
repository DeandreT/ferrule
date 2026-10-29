//! Occurrence-local conversion for the qualified flat XML double-target shape.

use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{Node, NodeId, Scope};
use roxmltree::Node as XmlNode;

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent, parse_u32, read_xsd_metadata_text};
use crate::float_cast_shape::{flat_schema, plain_scope};
use crate::resource::ResourceResolver;

pub(super) struct Witness {
    pub(super) index: usize,
    pub(super) input: u32,
    pub(super) feed: u32,
    pub(super) node: NodeId,
    pub(super) path: Vec<String>,
}

pub(super) fn install(
    target: &SchemaComponent,
    structure: &XmlNode<'_, '_>,
    resources: &ResourceResolver,
    builder: &mut GraphBuilder<'_>,
    scope: &mut Scope,
    witnesses: &[Witness],
) {
    if target.options.xml_scalar_output.is_some() {
        return;
    }
    let [source] = builder.sources else { return };
    if target.format != ComponentFormat::Xml
        || source.format != ComponentFormat::Xml
        || source.is_variable
        || target.is_variable
        || source.options.wsdl.is_some()
        || source.options.http_get.is_some()
        || source.options.local_xml_file_set
        || source.options.xml_schema_hints.is_some()
        || source.options.xml_root_view_read_policy
        || source.options.xml_allow_inactive_root_type_members
        || target.options.wsdl.is_some()
        || target.options.xml_schema_hints.is_some()
        || target.options.xml_root_view_read_policy
        || target.options.xml_allow_inactive_root_type_members
        || !plain_scope(scope)
        || !flat_schema(&source.schema, false)
        || !flat_schema(&target.schema, true)
    {
        return;
    }
    let Some(target_owner) = owner(structure, target.input_keys.iter().copied(), "inpkey") else {
        return;
    };
    let Some(source_owner) = owner(structure, source.output_keys.iter().copied(), "outkey") else {
        return;
    };
    if !ordinary_component(source_owner, &source.schema.name)
        || !ordinary_component(target_owner, &target.schema.name)
    {
        return;
    }
    let Some((target_root, target_document)) = entry_root(target_owner, &target.schema) else {
        return;
    };
    let Some((source_root, _)) = entry_root(source_owner, &source.schema) else {
        return;
    };
    if !matches!(
        target_document.attribute("casttotargettypemode"),
        None | Some("cast-in-subtree")
    ) {
        return;
    }
    let Some(target_types) = field_types(target_owner, &target.schema.name, resources) else {
        return;
    };
    let Some(source_types) = field_types(source_owner, &source.schema.name, resources) else {
        return;
    };
    let inputs = declarations(structure, "inpkey");
    let outputs = declarations(structure, "outkey");
    let Some(feeds) = raw_feeds(structure) else {
        return;
    };
    let source_fields = model_fields(&source.schema);
    let target_fields = model_fields(&target.schema);
    let mut binding_counts = BTreeMap::new();
    for binding in &scope.bindings {
        *binding_counts
            .entry(binding.target_field.clone())
            .or_insert(0_usize) += 1;
    }
    for witness in witnesses {
        let [field] = witness.path.as_slice() else {
            continue;
        };
        let Some(binding) = scope.bindings.get(witness.index) else {
            continue;
        };
        if binding.node != witness.node
            || binding.target_field != *field
            || target_types.get(field).map(String::as_str) != Some("double")
            || !target_fields.get(field.as_str()).is_some_and(|node| {
                node.repeating
                    && matches!(
                        node.kind,
                        SchemaKind::Scalar {
                            ty: ScalarType::Float
                        }
                    )
            })
            || binding_counts.get(field).copied().unwrap_or(0) < 2
            || !target.unambiguous_declared_input(witness.input)
            || !source.unambiguous_declared_output(witness.feed)
            || !feeds
                .get(&witness.input)
                .is_some_and(|from| from.len() == 1 && from.contains(&witness.feed))
        {
            continue;
        }
        let Some(Node::SourceField { path, frame: None }) = builder.graph.nodes.get(&witness.node)
        else {
            continue;
        };
        let [source_field] = path.as_slice() else {
            continue;
        };
        if source_types.get(source_field).map(String::as_str) != Some("string")
            || !source_fields
                .get(source_field.as_str())
                .is_some_and(|node| {
                    matches!(
                        node.kind,
                        SchemaKind::Scalar {
                            ty: ScalarType::String
                        }
                    ) && !node.nillable
                        && !node.xml_optional
                })
            || !direct_port(source_root, source_field, witness.feed, &outputs)
            || !direct_port(target_root, field, witness.input, &inputs)
        {
            continue;
        }
        let node = builder.alloc(Node::Call {
            function: "coerce_float".into(),
            args: vec![witness.node],
        });
        scope.bindings[witness.index].node = node;
    }
}

fn owner<'a, 'input>(
    structure: &XmlNode<'a, 'input>,
    keys: impl Iterator<Item = u32>,
    direction: &str,
) -> Option<XmlNode<'a, 'input>> {
    let keys = keys.collect::<BTreeSet<_>>();
    let owners = structure
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter(|entry| {
            parse_u32(entry.attribute(direction)).is_some_and(|key| keys.contains(&key))
        })
        .filter_map(|entry| {
            entry
                .ancestors()
                .find(|node| node.has_tag_name("component"))
        })
        .map(|owner| (owner.range().start, owner))
        .collect::<BTreeMap<_, _>>();
    let mut owners = owners.values();
    let owner = *owners.next()?;
    if owners.next().is_some() {
        return None;
    }
    (owner.attribute("library") == Some("xml") && owner.attribute("kind") == Some("14"))
        .then_some(owner)
}

pub(super) fn ordinary_component(owner: XmlNode<'_, '_>, root_name: &str) -> bool {
    ordinary_component_with(owner, root_name, &[])
}

/// `document_attributes` names further document metadata that has no output
/// semantics for the caller's profile; such a profile also accepts an empty
/// WSDL placeholder.
pub(super) fn ordinary_component_with(
    owner: XmlNode<'_, '_>,
    root_name: &str,
    document_attributes: &[&str],
) -> bool {
    if owner.tag_name().namespace().is_some()
        || owner
            .attributes()
            .any(|attribute| !matches!(attribute.name(), "name" | "library" | "kind" | "uid"))
        || owner.children().any(|child| {
            child.is_element() && !matches!(child.tag_name().name(), "view" | "properties" | "data")
        })
    {
        return false;
    }
    let properties = owner
        .children()
        .filter(|child| child.has_tag_name("properties"))
        .collect::<Vec<_>>();
    if properties.len() > 1
        || properties.iter().any(|properties| {
            properties.children().any(|child| child.is_element())
                || properties
                    .attributes()
                    .any(|attribute| match attribute.name() {
                        "XSLTDefaultOutput" | "AddSchemaLocation" => {
                            !matches!(attribute.value(), "0" | "1")
                        }
                        "XSLTTargetEncoding" => !attribute.value().eq_ignore_ascii_case("utf-8"),
                        _ => true,
                    })
        })
    {
        return false;
    }
    let Some(data) = unique_child(owner, "data") else {
        return false;
    };
    if data.children().any(|child| {
        child.is_element()
            && !matches!(child.tag_name().name(), "root" | "document")
            // An empty WSDL placeholder carries no settings.
            && !(!document_attributes.is_empty()
                && child.has_tag_name("wsdl")
                && child.attributes().len() == 0
                && !child.has_children())
    }) {
        return false;
    }
    let Some(document) = unique_child(data, "document") else {
        return false;
    };
    if document.attributes().any(|attribute| {
        !matches!(
            attribute.name(),
            "schema" | "instanceroot" | "inputinstance" | "outputinstance"
        ) && !document_attributes.contains(&attribute.name())
    }) {
        return false;
    }
    document
        .attribute("instanceroot")
        .is_none_or(|name| name == root_name || name == format!("{{}}{root_name}"))
}

pub(super) fn entry_root<'a, 'input>(
    owner: XmlNode<'a, 'input>,
    schema: &SchemaNode,
) -> Option<(XmlNode<'a, 'input>, XmlNode<'a, 'input>)> {
    let data = unique_child(owner, "data")?;
    let root = unique_child(data, "root")?;
    let wrappers = root
        .descendants()
        .filter(|entry| super::schema::is_xml_document_wrapper(entry, root))
        .collect::<Vec<_>>();
    let [document] = wrappers.as_slice() else {
        return None;
    };
    let entries = document
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    let [entry] = entries.as_slice() else {
        return None;
    };
    if !entry.has_tag_name("entry")
        || entry.attribute("name") != Some(schema.name.as_str())
        || !unqualified_entry(*entry, root)
        || entry
            .attribute("ferrule-kind")
            .is_some_and(|value| value != "group")
        || entry
            .attribute("ferrule-repeating")
            .is_some_and(|value| value != "0")
        || entry.attribute("datatype").is_some()
    {
        return None;
    }
    // No target/source node functions, defaults, conditions or cast overrides.
    for node in root.descendants().filter(|node| node.has_tag_name("entry")) {
        if node.attributes().any(|attribute| {
            !matches!(
                attribute.name(),
                "name"
                    | "ns"
                    | "expanded"
                    | "inpkey"
                    | "outkey"
                    | "clone"
                    | "casttotargettypemode"
                    | "ferrule-kind"
                    | "ferrule-repeating"
                    | "datatype"
            )
        }) || node
            .children()
            .any(|child| child.is_element() && !child.has_tag_name("entry"))
            || node.attribute("casttotargettypemode").is_some() && node != *document
        {
            return None;
        }
    }
    let fields = model_fields(schema);
    for child in entry.children().filter(|node| node.is_element()) {
        let field = *fields.get(child.attribute("name")?)?;
        let SchemaKind::Scalar { ty } = field.kind else {
            return None;
        };
        let datatype = match ty {
            ScalarType::String => "string",
            ScalarType::Int => "integer",
            ScalarType::Float => "decimal",
            ScalarType::Bool => "boolean",
        };
        if !child.has_tag_name("entry")
            || !unqualified_entry(child, root)
            || child.children().any(|node| node.is_element())
            || child
                .attribute("ferrule-kind")
                .is_some_and(|value| value != "scalar")
            || child
                .attribute("ferrule-repeating")
                .is_some_and(|value| value != if field.repeating { "1" } else { "0" })
            || child
                .attribute("datatype")
                .is_some_and(|value| value != datatype)
            || child.attribute("ns").is_some_and(|value| value != "0")
        {
            return None;
        }
    }
    Some((*entry, *document))
}

fn unique_child<'a, 'input>(node: XmlNode<'a, 'input>, name: &str) -> Option<XmlNode<'a, 'input>> {
    let nodes = node
        .children()
        .filter(|node| node.has_tag_name(name))
        .collect::<Vec<_>>();
    let [node] = nodes.as_slice() else {
        return None;
    };
    Some(*node)
}

fn direct_port(
    root: XmlNode<'_, '_>,
    name: &str,
    key: u32,
    declarations: &BTreeMap<u32, Vec<XmlNode<'_, '_>>>,
) -> bool {
    let Some(entries) = declarations.get(&key) else {
        return false;
    };
    let [entry] = entries.as_slice() else {
        return false;
    };
    entry.has_tag_name("entry")
        && entry.parent() == Some(root)
        && entry.attribute("name") == Some(name)
        && !entry.children().any(|node| node.is_element())
}

fn unqualified_entry(entry: XmlNode<'_, '_>, root: XmlNode<'_, '_>) -> bool {
    let Some(slot) = entry.attribute("ns").unwrap_or("0").parse::<usize>().ok() else {
        return false;
    };
    let Some(header) = unique_child(root, "header") else {
        return false;
    };
    let Some(namespaces) = unique_child(header, "namespaces") else {
        return false;
    };
    if namespaces
        .children()
        .any(|node| node.is_element() && !node.has_tag_name("namespace"))
    {
        return false;
    }
    namespaces
        .children()
        .filter(|node| node.has_tag_name("namespace"))
        .nth(slot)
        .is_some_and(|namespace| namespace.attribute("uid").unwrap_or_default().is_empty())
}

fn raw_feeds(structure: &XmlNode<'_, '_>) -> Option<BTreeMap<u32, BTreeSet<u32>>> {
    let graph = unique_child(*structure, "graph")?;
    let vertices = unique_child(graph, "vertices")?;
    let mut feeds: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for vertex in vertices
        .children()
        .filter(|node| node.has_tag_name("vertex"))
    {
        let from = parse_u32(vertex.attribute("vertexkey"))?;
        for edge in vertex
            .children()
            .filter(|node| node.has_tag_name("edges"))
            .flat_map(|edges| edges.children().filter(|node| node.has_tag_name("edge")))
        {
            let to = parse_u32(edge.attribute("vertexkey"))?;
            feeds.entry(to).or_default().insert(from);
        }
    }
    Some(feeds)
}

fn declarations<'a, 'input>(
    structure: &XmlNode<'a, 'input>,
    direction: &str,
) -> BTreeMap<u32, Vec<XmlNode<'a, 'input>>> {
    let mut result: BTreeMap<u32, Vec<XmlNode<'a, 'input>>> = BTreeMap::new();
    for node in structure.descendants() {
        if let Some(key) = parse_u32(node.attribute(direction)) {
            result.entry(key).or_default().push(node);
        }
    }
    result
}

fn model_fields(schema: &SchemaNode) -> BTreeMap<&str, &SchemaNode> {
    match &schema.kind {
        SchemaKind::Group { children, .. } => children
            .iter()
            .map(|node| (node.name.as_str(), node))
            .collect(),
        _ => BTreeMap::new(),
    }
}

pub(super) fn field_types(
    owner: XmlNode<'_, '_>,
    root_name: &str,
    resources: &ResourceResolver,
) -> Option<BTreeMap<String, String>> {
    let data = unique_child(owner, "data")?;
    let reference = unique_child(data, "document")?.attribute("schema")?;
    let text = read_xsd_metadata_text(
        resources,
        reference,
        "target float cast schema",
        8 * 1024 * 1024,
    )
    .ok()??;
    field_types_text(&text, root_name)
}

pub(super) fn field_types_text(text: &str, root_name: &str) -> Option<BTreeMap<String, String>> {
    let document = roxmltree::Document::parse(text).ok()?;
    let schema = document.root_element();
    const XSD: &str = "http://www.w3.org/2001/XMLSchema";
    if !schema.has_tag_name((XSD, "schema")) || schema.attribute("targetNamespace").is_some() {
        return None;
    }
    let roots = schema
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return None;
    };
    if !root.has_tag_name((XSD, "element"))
        || root.attribute("name") != Some(root_name)
        || root.attribute("type").is_some()
    {
        return None;
    }
    let complex = unique_xsd_child(*root, "complexType")?;
    let sequence = unique_xsd_child(complex, "sequence")?;
    if complex.children().filter(|node| node.is_element()).count() != 1 {
        return None;
    }
    let mut types = BTreeMap::new();
    for field in sequence.children().filter(|node| node.is_element()) {
        if !field.has_tag_name((XSD, "element"))
            || field.children().any(|node| node.is_element())
            || field.attributes().any(|attribute| {
                !matches!(
                    attribute.name(),
                    "name" | "type" | "minOccurs" | "maxOccurs" | "nillable"
                )
            })
        {
            return None;
        }
        let name = field.attribute("name")?;
        let (prefix, ty) = field.attribute("type")?.split_once(':')?;
        if field.lookup_namespace_uri(Some(prefix)) != Some(XSD)
            || types.insert(name.into(), ty.into()).is_some()
        {
            return None;
        }
    }
    Some(types)
}

fn unique_xsd_child<'a, 'input>(
    node: XmlNode<'a, 'input>,
    name: &str,
) -> Option<XmlNode<'a, 'input>> {
    let children = node
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    let [child] = children.as_slice() else {
        return None;
    };
    child
        .has_tag_name(("http://www.w3.org/2001/XMLSchema", name))
        .then_some(*child)
}
