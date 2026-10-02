//! A declared XML preview input can be unused by a complete constant mapping.
//! This proof is deliberately narrower than ordinary port-based role inference.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::PathBuf;

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{
    FormatOptions, IterationOutput, Node, Project, ScopeConstruction, ScopeIteration,
    SortFilterOrder,
};
use roxmltree::Node as XmlNode;

pub(super) struct BoundaryWitness<'a, 'input> {
    pub(super) component: XmlNode<'a, 'input>,
    pub(super) schema: SchemaNode,
    pub(super) schema_file_authoritative: bool,
    pub(super) authoritative_schema_path: Option<PathBuf>,
    pub(super) no_port_warning_index: Option<usize>,
}

/// Avoid retaining schema copies for ordinary connected designs. This is only a
/// cheap inventory prefilter; the final proof still checks every owner and feed.
pub(super) fn may_contain_unused_input(structure: XmlNode<'_, '_>) -> bool {
    let Some(children) = sole_child(structure, "children") else {
        return false;
    };
    let components: Vec<_> = children
        .children()
        .filter(XmlNode::is_element)
        .take(4)
        .collect();
    components.len() == 3
        && components.iter().any(|component| {
            component.attribute("library") == Some("xml")
                && explicit_role(*component) == Some(Role::Input)
                && !component
                    .descendants()
                    .any(|n| n.attribute("inpkey").is_some() || n.attribute("outkey").is_some())
        })
}

/// Remove only the originating inventory diagnostic after proving explicit
/// ownership and complete static construction. Other diagnostics are untouched.
pub(super) fn classify(
    mapping: &XmlNode<'_, '_>,
    structure: &XmlNode<'_, '_>,
    boundaries: &[BoundaryWitness<'_, '_>],
    project: &Project,
    warnings: &mut Vec<String>,
) {
    if let Some(index) = unused_input_warning(mapping, structure, boundaries, project, warnings) {
        warnings.remove(index);
    }
}

fn unused_input_warning(
    mapping: &XmlNode<'_, '_>,
    structure: &XmlNode<'_, '_>,
    boundaries: &[BoundaryWitness<'_, '_>],
    project: &Project,
    warnings: &[String],
) -> Option<usize> {
    if boundaries.len() != 2
        || warnings.len() != 1
        || mapping
            .descendants()
            .any(|n| n.has_tag_name("conditions") || n.has_tag_name("condition"))
    {
        return None;
    }
    let children = sole_child(*structure, "children")?;
    let components: Vec<_> = children
        .children()
        .filter(XmlNode::is_element)
        .take(4)
        .collect();
    if components.len() != 3 || components.iter().any(|c| !c.has_tag_name("component")) {
        return None;
    }
    let source = boundaries
        .iter()
        .find(|b| explicit_role(b.component) == Some(Role::Input))?;
    let target = boundaries
        .iter()
        .find(|b| explicit_role(b.component) == Some(Role::Output))?;
    if source.component == target.component
        || !source.schema_file_authoritative
        || !target.schema_file_authoritative
        || source.schema != project.source
        || target.schema != project.target
        || !ordinary_boundary(source)
        || !ordinary_boundary(target)
        || source
            .component
            .children()
            .any(|n| n.has_tag_name("properties") && n.attribute("XSLTDefaultOutput") == Some("1"))
    {
        return None;
    }
    let index = source.no_port_warning_index?;
    if index >= warnings.len()
        || target.no_port_warning_index.is_some()
        || source
            .component
            .descendants()
            .any(|n| n.attribute("inpkey").is_some() || n.attribute("outkey").is_some())
    {
        return None;
    }
    let expression = components
        .iter()
        .copied()
        .find(|c| *c != source.component && *c != target.component)?;
    // Every main component is accounted for; no second boundary or hidden control.
    if !components.contains(&source.component) || !components.contains(&target.component) {
        return None;
    }
    let (field, ty) = plain_scalar_document(&project.target)?;
    plain_scalar_document(&project.source)?;
    // Projection intentionally omits some XSD constraints. An unconstrained IR
    // scalar alone cannot prove that either physical boundary is plain.
    if !plain_physical_schema(source) || !plain_physical_schema(target) {
        return None;
    }
    let target_payload = sole_payload(target.component)?;
    let leaf = sole_child(target_payload, "entry")?;
    if leaf.attribute("name") != Some(field)
        || leaf.children().any(|n| n.is_element())
        || target
            .component
            .descendants()
            .any(|n| n.attribute("outkey").is_some())
    {
        return None;
    }
    let input = key(leaf.attribute("inpkey"))?;
    if target
        .component
        .descendants()
        .filter(|n| n.attribute("inpkey").is_some())
        .count()
        != 1
    {
        return None;
    }
    let output = expression_output(expression)?;
    let is_function = expression.attribute("library") == Some("user");
    if !complete_expression(*mapping, expression)
        || !unique_identifiers(
            *mapping,
            if is_function { 7 } else { 4 },
            if is_function { 4 } else { 2 },
        )
    {
        return None;
    }
    if !exact_edge(*structure, output, input) || !static_constant_target(project, field, ty) {
        return None;
    }
    Some(index)
}

#[derive(PartialEq, Eq)]
enum Role {
    Input,
    Output,
}

fn explicit_role(component: XmlNode<'_, '_>) -> Option<Role> {
    let data = sole_child(component, "data")?;
    let document = sole_child(data, "document")?;
    match (
        document.attribute("inputinstance"),
        document.attribute("outputinstance"),
    ) {
        (Some(path), None) if static_path(path) => Some(Role::Input),
        (None, Some(path)) if static_path(path) => Some(Role::Output),
        _ => None,
    }
}

fn static_path(path: &str) -> bool {
    !path.trim().is_empty() && !path.contains(['*', '?']) && !path.contains("://")
}

fn ordinary_boundary(boundary: &BoundaryWitness<'_, '_>) -> bool {
    let component = boundary.component;
    if component.attribute("library") != Some("xml") || component.attribute("kind") != Some("14") {
        return false;
    }
    let Some(data) = sole_child(component, "data") else {
        return false;
    };
    if data
        .children()
        .filter(XmlNode::is_element)
        .any(|n| !n.has_tag_name("root") && !n.has_tag_name("document"))
        || component.descendants().any(|n| {
            n.has_tag_name("conditions")
                || n.has_tag_name("condition")
                || n.has_tag_name("parameter")
                || n.has_tag_name("file")
        })
        || component.children().any(|n| {
            n.has_tag_name("properties") && n.attribute("PassThrough").is_some_and(|v| v != "0")
        })
    {
        return false;
    }
    let Some(document) = sole_child(data, "document") else {
        return false;
    };
    if !document
        .attribute("schema")
        .is_some_and(|s| !s.trim().is_empty())
    {
        return false;
    }
    let expected_root = format!("{{}}{}", boundary.schema.name);
    if document.attribute("instanceroot") != Some(expected_root.as_str()) {
        return false;
    }
    let Some(payload) = sole_payload(component) else {
        return false;
    };
    let Some((field, _)) = plain_scalar_document(&boundary.schema) else {
        return false;
    };
    let Some(leaf) = sole_child(payload, "entry") else {
        return false;
    };
    payload.attribute("name") == Some(boundary.schema.name.as_str())
        && unqualified_entry(payload)
        && unqualified_entry(leaf)
        && leaf.attribute("name") == Some(field)
        && !leaf.children().any(|n| n.is_element())
}

fn sole_payload<'a, 'input>(component: XmlNode<'a, 'input>) -> Option<XmlNode<'a, 'input>> {
    let data = sole_child(component, "data")?;
    let root = sole_child(data, "root")?;
    let wrapper = sole_child(root, "entry")?;
    let header = sole_child(root, "header")?;
    let namespaces = sole_child(header, "namespaces")?;
    let empty_namespace = namespaces
        .children()
        .find(|n| n.has_tag_name("namespace"))?;
    if empty_namespace
        .attribute("uid")
        .is_some_and(|uid| !uid.is_empty())
    {
        return None;
    }
    let document = super::schema::xml_document_wrapper(root)?;
    // The proof accepts one file/document chain, never a second payload root.
    if wrapper != document && sole_child(wrapper, "entry")? != document {
        return None;
    }
    sole_child(document, "entry")
}

fn unqualified_entry(entry: XmlNode<'_, '_>) -> bool {
    entry.attribute("ns").is_none_or(|slot| slot == "0")
}

fn plain_scalar_document(schema: &SchemaNode) -> Option<(&str, ScalarType)> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    let [field] = children.as_slice() else {
        return None;
    };
    let SchemaKind::Scalar { ty } = field.kind else {
        return None;
    };
    if *schema != SchemaNode::group(schema.name.clone(), children.clone())
        || *field != SchemaNode::scalar(field.name.clone(), ty)
    {
        return None;
    }
    Some((&field.name, ty))
}

/// A single-resource physical witness, independent of the lossy XSD projection.
/// Only one inline sequence containing one ordinary builtin scalar is admitted.
/// Aliases, facets, imported resources and all unproven metadata retain the warning.
fn plain_physical_schema(boundary: &BoundaryWitness<'_, '_>) -> bool {
    const XSD: &str = "http://www.w3.org/2001/XMLSchema";
    const MAX_BYTES: u64 = 8192;
    let Some((field, ty)) = plain_scalar_document(&boundary.schema) else {
        return false;
    };
    let Some(path) = &boundary.authoritative_schema_path else {
        return false;
    };
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file.take(MAX_BYTES + 1).read_to_end(&mut bytes).is_err() || bytes.len() as u64 > MAX_BYTES {
        return false;
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return false;
    };
    // Do not enable DTD/entity loading, even for an otherwise plain projection.
    let Ok(document) = roxmltree::Document::parse(text) else {
        return false;
    };
    if document.descendants().take(65).count() > 64 {
        return false;
    }
    let root = document.root_element();
    if !root.has_tag_name((XSD, "schema"))
        || !plain_attributes(root, &["elementFormDefault", "attributeFormDefault"])
        || ["elementFormDefault", "attributeFormDefault"]
            .iter()
            .any(|name| root.attribute(*name).is_some_and(|v| v != "unqualified"))
    {
        return false;
    }
    let Some(element) = plain_xsd_child(root, "element") else {
        return false;
    };
    if !plain_attributes(element, &["name"])
        || element.attribute("name") != Some(boundary.schema.name.as_str())
    {
        return false;
    }
    let Some(complex) = plain_xsd_child(element, "complexType") else {
        return false;
    };
    let Some(sequence) = plain_xsd_child(complex, "sequence") else {
        return false;
    };
    let Some(leaf) = plain_xsd_child(sequence, "element") else {
        return false;
    };
    if !plain_attributes(complex, &[])
        || !plain_attributes(sequence, &["minOccurs", "maxOccurs"])
        || !plain_attributes(leaf, &["name", "type", "minOccurs", "maxOccurs"])
        || [sequence, leaf].iter().any(|node| {
            ["minOccurs", "maxOccurs"]
                .iter()
                .any(|name| node.attribute(*name).is_some_and(|v| v != "1"))
        })
        || leaf.attribute("name") != Some(field)
        || leaf.children().any(|node| node.is_element())
        || !plain_xml_content(leaf)
    {
        return false;
    }
    let Some(type_name) = leaf.attribute("type") else {
        return false;
    };
    let (prefix, local) = type_name
        .split_once(':')
        .map_or((None, type_name), |(prefix, local)| (Some(prefix), local));
    if leaf.lookup_namespace_uri(prefix) != Some(XSD) {
        return false;
    }
    matches!(
        (ty, local),
        (ScalarType::String, "string")
            | (ScalarType::Int, "integer")
            | (ScalarType::Float, "decimal")
            | (ScalarType::Bool, "boolean")
    )
}

fn plain_attributes(node: XmlNode<'_, '_>, allowed: &[&str]) -> bool {
    node.attributes()
        .all(|a| a.namespace().is_none() && allowed.contains(&a.name()))
}

fn plain_xml_content(node: XmlNode<'_, '_>) -> bool {
    node.children().all(|child| {
        child.is_element()
            || child.is_comment()
            || child.is_pi()
            || child.text().is_some_and(|text| text.trim().is_empty())
    })
}

fn plain_xsd_child<'a, 'input>(
    node: XmlNode<'a, 'input>,
    name: &str,
) -> Option<XmlNode<'a, 'input>> {
    if !plain_xml_content(node) {
        return None;
    }
    let mut children = node.children().filter(XmlNode::is_element);
    let child = children.next()?;
    (child.has_tag_name(("http://www.w3.org/2001/XMLSchema", name)) && children.next().is_none())
        .then_some(child)
}

fn expression_output(component: XmlNode<'_, '_>) -> Option<u32> {
    match (component.attribute("library"), component.attribute("kind")) {
        (Some("core"), Some("2")) => {
            if component.children().any(|n| n.has_tag_name("sources")) {
                return None;
            }
            let targets = sole_child(component, "targets")?;
            let pin = sole_child(targets, "datapoint")?;
            if pin.attribute("pos") != Some("0") {
                return None;
            }
            let data = sole_child(component, "data")?;
            sole_child(data, "constant")?;
            if data.children().filter(XmlNode::is_element).count() != 1
                || targets.children().filter(XmlNode::is_element).count() != 1
            {
                return None;
            }
            key(pin.attribute("key"))
        }
        (Some("user"), Some("19")) => {
            if component
                .children()
                .any(|n| n.has_tag_name("sources") || n.has_tag_name("targets"))
            {
                return None;
            }
            let data = sole_child(component, "data")?;
            if data.children().filter(XmlNode::is_element).count() != 2 {
                return None;
            }
            let roots: Vec<_> = data.children().filter(|n| n.has_tag_name("root")).collect();
            let [input, output] = roots.as_slice() else {
                return None;
            };
            if input.descendants().any(|n| n.has_tag_name("entry")) {
                return None;
            }
            let entry = sole_child(*output, "entry")?;
            if entry.attribute("inpkey").is_some()
                || entry.children().any(|n| n.is_element())
                || component
                    .descendants()
                    .filter(|n| n.attribute("outkey").is_some())
                    .count()
                    != 1
            {
                return None;
            }
            key(entry.attribute("componentid"))?;
            key(entry.attribute("outkey"))
        }
        _ => None,
    }
}

fn complete_expression(mapping: XmlNode<'_, '_>, expression: XmlNode<'_, '_>) -> bool {
    let definitions: Vec<_> = mapping
        .children()
        .filter(|n| n.has_tag_name("component") && n.attribute("library").is_some())
        .take(2)
        .collect();
    if expression.attribute("library") == Some("core") {
        return definitions.is_empty();
    }
    let [definition] = definitions.as_slice() else {
        return false;
    };
    if definition.attribute("library") != Some("user")
        || definition.attribute("name") != expression.attribute("name")
    {
        return false;
    }
    let Some(structure) = sole_child(*definition, "structure") else {
        return false;
    };
    let Some(children) = sole_child(structure, "children") else {
        return false;
    };
    let parts: Vec<_> = children
        .children()
        .filter(XmlNode::is_element)
        .take(3)
        .collect();
    if parts.len() != 2
        || parts
            .iter()
            .any(|n| !n.has_tag_name("component") || n.attribute("library") != Some("core"))
    {
        return false;
    }
    let Some(constant) = parts
        .iter()
        .copied()
        .find(|n| n.attribute("kind") == Some("2"))
    else {
        return false;
    };
    let Some(output) = parts
        .iter()
        .copied()
        .find(|n| n.attribute("kind") == Some("7"))
    else {
        return false;
    };
    let Some(from) = expression_output(constant) else {
        return false;
    };
    let Some(sources) = sole_child(output, "sources") else {
        return false;
    };
    let Some(pin) = sole_child(sources, "datapoint") else {
        return false;
    };
    let Some(to) = key(pin.attribute("key")) else {
        return false;
    };
    let Some(data) = sole_child(output, "data") else {
        return false;
    };
    let Some(parameter) = sole_child(data, "parameter") else {
        return false;
    };
    if pin.attribute("pos") != Some("0")
        || sole_child(data, "output").is_none()
        || data.children().filter(XmlNode::is_element).count() != 2
        || sources.children().filter(XmlNode::is_element).count() != 1
        || parameter.attribute("usageKind") != Some("output")
        || parameter.attribute("name") != output.attribute("name")
        || output.children().any(|n| n.has_tag_name("targets"))
    {
        return false;
    }
    let Some(call_data) = sole_child(expression, "data") else {
        return false;
    };
    let Some(entry) = call_data
        .children()
        .filter(|n| n.has_tag_name("root"))
        .nth(1)
        .and_then(|root| sole_child(root, "entry"))
    else {
        return false;
    };
    entry.attribute("name") == output.attribute("name")
        && key(entry.attribute("componentid")) == key(output.attribute("uid"))
        && exact_edge(structure, from, to)
}

fn exact_edge(structure: XmlNode<'_, '_>, output: u32, input: u32) -> bool {
    if structure.children().any(|n| n.has_tag_name("connections")) {
        return false;
    }
    let Some(graph) = sole_child(structure, "graph") else {
        return false;
    };
    if graph.attribute("directed") != Some("1") {
        return false;
    }
    let mut edge_lists = graph.children().filter(|n| n.has_tag_name("edges"));
    if let Some(edges) = edge_lists.next()
        && edges.children().any(|n| n.is_element())
    {
        return false;
    }
    if edge_lists.next().is_some() {
        return false;
    }
    let Some(vertices) = sole_child(graph, "vertices") else {
        return false;
    };
    let Some(vertex) = sole_child(vertices, "vertex") else {
        return false;
    };
    let Some(vertex_edges) = sole_child(vertex, "edges") else {
        return false;
    };
    let Some(edge) = sole_child(vertex_edges, "edge") else {
        return false;
    };
    key(vertex.attribute("vertexkey")) == Some(output)
        && key(edge.attribute("vertexkey")) == Some(input)
        && !edge.children().any(|n| n.is_element())
        && graph
            .descendants()
            .filter(|n| n.has_tag_name("vertex"))
            .count()
            == 1
        && graph
            .descendants()
            .filter(|n| n.has_tag_name("edge"))
            .count()
            == 1
}

fn static_constant_target(project: &Project, field: &str, ty: ScalarType) -> bool {
    let root = &project.root;
    let [binding] = root.bindings.as_slice() else {
        return false;
    };
    let ordinary_xml = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    if project.source_options != ordinary_xml
        || project.target_options != ordinary_xml
        || !project.extra_sources.is_empty()
        || !project.extra_targets.is_empty()
        || !project.failure_rules.is_empty()
        || !root.target_field.is_empty()
        || !matches!(root.iteration, ScopeIteration::None)
        || root.construction != ScopeConstruction::Constructed
        || root.filter.is_some()
        || root.post_group_filter.is_some()
        || root.has_grouping()
        || root.has_sort()
        || root.sort_descending
        || root.sort_filter_order != SortFilterOrder::default()
        || !root.windows.is_empty()
        || root.iteration_output != IterationOutput::Repeated
        || !root.dynamic_bindings.is_empty()
        || !root.children.is_empty()
        || !root.dynamic_children.is_empty()
        || root.merge_dynamic_fields
        || binding.target_field != field
        || !engine::validate(project).is_empty()
    {
        return false;
    }
    let value = match project.graph.nodes.get(&binding.node) {
        Some(Node::Const { value }) if project.user_functions.is_empty() => value,
        Some(Node::UserFunctionCall { function, args }) if args.is_empty() => {
            let Some(function) = project.user_functions.get(function) else {
                return false;
            };
            if project.user_functions.len() != 1
                || !function.parameters.is_empty()
                || function.output_type != ty
            {
                return false;
            }
            let Some(Node::Const { value }) = function.body.nodes.get(&function.output) else {
                return false;
            };
            value
        }
        _ => return false,
    };
    matches!(
        (ty, value),
        (ScalarType::String, Value::String(_))
            | (ScalarType::Int, Value::Int(_))
            | (ScalarType::Bool, Value::Bool(_))
    ) || matches!((ty, value), (ScalarType::Float, Value::Float(n)) if n.is_finite())
}

fn unique_identifiers(
    mapping: XmlNode<'_, '_>,
    expected_components: usize,
    expected_ports: usize,
) -> bool {
    let mut components = BTreeSet::new();
    let mut ports = BTreeSet::new();
    let mut component_count = 0;
    for node in mapping.descendants().filter(XmlNode::is_element) {
        component_count += usize::from(node.has_tag_name("component"));
        if component_count > expected_components {
            return false;
        }
        let named_definition = node.parent() == Some(mapping)
            && node.attribute("library") == Some("user")
            && node.attribute("uid").is_none();
        if node.has_tag_name("component")
            && !named_definition
            && !key(node.attribute("uid")).is_some_and(|id| components.insert(id))
        {
            return false;
        }
        for attribute in node.attributes() {
            if ports.len() > expected_ports {
                return false;
            }
            if (matches!(attribute.name(), "inpkey" | "outkey")
                || node.has_tag_name("datapoint") && attribute.name() == "key")
                && !key(Some(attribute.value())).is_some_and(|id| ports.insert(id))
            {
                return false;
            }
        }
    }
    component_count == expected_components && ports.len() == expected_ports
}

fn key(value: Option<&str>) -> Option<u32> {
    let value = value?;
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok().filter(|key| *key != 0)
}

fn sole_child<'a, 'input>(parent: XmlNode<'a, 'input>, name: &str) -> Option<XmlNode<'a, 'input>> {
    let mut children = parent.children().filter(|n| n.has_tag_name(name));
    let child = children.next()?;
    children.next().is_none().then_some(child)
}
