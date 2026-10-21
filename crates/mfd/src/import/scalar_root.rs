//! Bounded singleton scalar document-root construction.

use std::collections::BTreeSet;

use ir::{EffectiveSchemaView, ScalarType, SchemaKind, SchemaNode};
use mapping::ScopeConstruction;

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent, parse_u32, read_xsd_metadata_text};
use super::scope::ScopeBuilder;
use crate::resource::ResourceResolver;

#[cfg(test)]
mod tests;

pub(super) fn install(
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    let feeds = target
        .ports
        .iter()
        .filter(|(key, path)| path.is_empty() && target.input_keys.contains(key))
        .filter_map(|(key, _)| builder.edge_from.get(key).copied())
        .collect::<BTreeSet<_>>();
    if feeds.is_empty() || matches!(target.schema.kind, SchemaKind::Group { .. }) {
        return;
    }
    let result = prepare(target, structure, resources, &feeds, builder, scopes);
    match result {
        Ok(feed) => {
            if let Some(value) = builder.binding_node_at_anchor(feed, &[], &[]) {
                scopes.root.construction = ScopeConstruction::Scalar { value };
            } else {
                refuse(builder, target, "feed cannot be materialized as a scalar");
            }
        }
        Err(reason) => refuse(builder, target, &reason),
    }
}

fn refuse(builder: &mut GraphBuilder<'_>, target: &SchemaComponent, reason: &str) {
    builder.warnings.push(format!(
        "connection into a scalar document root is not supported; binding skipped: `{}`: {reason}",
        target.schema.name
    ));
}

fn prepare(
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    feeds: &BTreeSet<u32>,
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> Result<u32, String> {
    let SchemaKind::Scalar { ty } = target.schema.kind else {
        return Err("target is not a required non-null singleton XML scalar".into());
    };
    if target.format != ComponentFormat::Xml
        || !required_singleton(&target.schema)
        || target.options.local_xml_file_set
        || target.ports.iter().any(|(key, path)| {
            path.as_slice() == [super::schema::TARGET_DOCUMENT_PATH_PORT]
                && builder.edge_from.contains_key(key)
                && builder.static_string_feed(builder.edge_from[key]).is_none()
        })
        || scopes.root.iterates()
        || scopes.root.output_path().is_some()
        || !scopes.root.bindings.is_empty()
        || !scopes.root.children.is_empty()
        || !scopes.root.dynamic_bindings.is_empty()
        || !scopes.root.dynamic_children.is_empty()
        || scopes.root.merge_dynamic_fields
    {
        return Err("target is not a required non-null singleton XML scalar".into());
    }
    if target.schema.fixed.is_some() || target.schema.default.is_some() {
        return Err("root metadata requires an unqualified transformation".into());
    }
    let component = structure
        .descendants()
        .filter(|node| node.has_tag_name("component"))
        .find(|component| {
            component
                .descendants()
                .filter(|node| node.has_tag_name("entry"))
                .filter_map(|entry| parse_u32(entry.attribute("inpkey")))
                .any(|key| target.input_keys.contains(&key))
        })
        .ok_or("root owner metadata is unavailable")?;
    if !raw_required_path(component, resources, &target.schema.name, &[], false) {
        return Err("target is not a required non-null singleton XML scalar".into());
    }
    if component.descendants().any(|node| {
        node.has_tag_name("inputnodefunctions") && node.children().any(|child| child.is_element())
            || node.attribute("casttotargettypemode").is_some()
    }) {
        return Err("root metadata requires an unqualified transformation".into());
    }
    if let Some(reference) = component
        .descendants()
        .find(|node| node.has_tag_name("document"))
        .and_then(|node| node.attribute("schema"))
    {
        let text = read_xsd_metadata_text(
            resources,
            reference,
            "scalar root XML Schema",
            8 * 1024 * 1024,
        )
        .map_err(|_| "root owner metadata is unavailable")?;
        if let Some(text) = text {
            let document = roxmltree::Document::parse(&text)
                .map_err(|_| "root owner metadata is unavailable")?;
            if super::target_node_function::path_requires_datetime(
                document.root_element(),
                &target.schema.name,
                &[],
            ) {
                return Err("root metadata requires an unqualified transformation".into());
            }
        }
    }
    if !raw_required_path(component, resources, &target.schema.name, &[], true) {
        return Err("root metadata requires an unqualified transformation".into());
    }
    if feeds.len() != 1 {
        return Err(format!(
            "expected one distinct root feed, found {:?}",
            feeds.iter().copied().collect::<Vec<_>>()
        ));
    }
    let feed = *feeds.first().expect("nonempty feed census");
    let owners = builder
        .sources
        .iter()
        .filter_map(|source| source.ports.get(&feed).map(|path| (*source, path)))
        .collect::<Vec<_>>();
    if let [(source, path)] = owners.as_slice() {
        if source.format != ComponentFormat::Xml
            || source.options.local_xml_file_set
            || !required_singleton(&source.schema)
        {
            return Err("source path is not a required singleton XML scalar".into());
        }
        let source_component = structure
            .descendants()
            .filter(|node| node.has_tag_name("component"))
            .find(|component| {
                component
                    .descendants()
                    .filter(|node| node.has_tag_name("entry"))
                    .filter_map(|entry| parse_u32(entry.attribute("outkey")))
                    .any(|key| source.output_keys.contains(&key))
            });
        if !source_component.is_some_and(|component| {
            raw_required_path(component, resources, &source.schema.name, path, true)
        }) {
            return Err("source path is not a required singleton XML scalar".into());
        }
        let mut node = &source.schema;
        for part in path.iter() {
            node = node
                .child(part)
                .ok_or("source path is not a required singleton XML scalar")?;
            if !required_singleton(node) {
                return Err("source path is not a required singleton XML scalar".into());
            }
        }
        let SchemaKind::Scalar { ty: source_ty } = node.kind else {
            return Err("source path is not a required singleton XML scalar".into());
        };
        if source_ty != ty {
            return Err("source and target scalar kinds differ".into());
        }
        if builder.has_source_node_functions(feed) {
            return Err("source node functions require an unqualified transformation".into());
        }
        return Ok(feed);
    }
    if !owners.is_empty() {
        return Err("source path is not a required singleton XML scalar".into());
    }
    let constant = builder
        .fn_by_output
        .get(&feed)
        .and_then(|index| builder.fn_components.get(*index));
    if !constant.is_some_and(|component| {
        component.library == "core"
            && component.kind == 2
            && component.name == "constant"
            && component
                .constant
                .as_ref()
                .is_some_and(|(value, datatype)| valid_constant(value, datatype, ty))
    }) {
        return Err("constant is invalid, null or has a different scalar kind".into());
    }
    Ok(feed)
}

// Contradictory role metadata has no effective view and is refused.
fn required_singleton(node: &SchemaNode) -> bool {
    #[cfg(test)]
    if tests::legacy_selected() {
        return tests::legacy_required_singleton(node);
    }
    node.effective_view()
        .is_ok_and(EffectiveSchemaView::is_required_non_null_singleton)
}

// XSD occurrence minima greater than one and annotations on a global root
// are not represented by SchemaNode's repeating/optional flags. Prove the
// selected declarations and their containing particles before scalar lowering.
fn raw_required_path(
    component: roxmltree::Node<'_, '_>,
    resources: &ResourceResolver,
    root: &str,
    path: &[String],
    primitive: bool,
) -> bool {
    let Some(reference) = component
        .descendants()
        .find(|node| node.has_tag_name("document"))
        .and_then(|node| node.attribute("schema"))
    else {
        return false;
    };
    let Ok(Some(text)) = read_xsd_metadata_text(
        resources,
        reference,
        "scalar root XML Schema",
        8 * 1024 * 1024,
    ) else {
        return false;
    };
    let Ok(document) = roxmltree::Document::parse(&text) else {
        return false;
    };
    let schema = document.root_element();
    let mut roots = schema
        .children()
        .filter(|node| xsd_node(*node, "element") && node.attribute("name") == Some(root));
    let Some(element) = roots.next() else {
        return false;
    };
    roots.next().is_none() && required_element_path(schema, element, path, primitive, 0)
}

fn xsd_node(node: roxmltree::Node<'_, '_>, name: &str) -> bool {
    node.has_tag_name(("http://www.w3.org/2001/XMLSchema", name))
}

fn occurs_once(node: roxmltree::Node<'_, '_>) -> bool {
    ["minOccurs", "maxOccurs"].into_iter().all(|attribute| {
        node.attribute(attribute).is_none_or(|value| {
            let digits = value.trim().strip_prefix('+').unwrap_or(value.trim());
            !digits.is_empty()
                && digits.bytes().all(|byte| byte.is_ascii_digit())
                && digits.trim_start_matches('0') == "1"
        })
    })
}

fn local_schema_name<'a>(
    schema: roxmltree::Node<'_, '_>,
    context: roxmltree::Node<'_, '_>,
    name: &'a str,
) -> Option<&'a str> {
    let (prefix, local) = name
        .split_once(':')
        .map_or((None, name), |(prefix, local)| (Some(prefix), local));
    let namespace = context
        .lookup_namespace_uri(prefix)
        .filter(|value| !value.is_empty());
    let own_namespace = schema
        .attribute("targetNamespace")
        .filter(|value| !value.is_empty());
    (namespace == own_namespace).then_some(local)
}

fn unique_declaration<'a, 'input>(
    schema: roxmltree::Node<'a, 'input>,
    context: roxmltree::Node<'_, '_>,
    kind: &str,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    let name = local_schema_name(schema, context, name)?;
    let mut declarations = schema
        .children()
        .filter(|node| xsd_node(*node, kind) && node.attribute("name") == Some(name));
    let first = declarations.next()?;
    declarations.next().is_none().then_some(first)
}

fn required_element_path<'a, 'input>(
    schema: roxmltree::Node<'a, 'input>,
    mut element: roxmltree::Node<'a, 'input>,
    path: &[String],
    primitive: bool,
    depth: usize,
) -> bool {
    if depth >= 128 || !occurs_once(element) {
        return false;
    }
    if let Some(reference) = element.attribute("ref") {
        let Some(resolved) = unique_declaration(schema, element, "element", reference) else {
            return false;
        };
        return required_element_path(schema, resolved, path, primitive, depth + 1);
    }
    let Some((name, rest)) = path.split_first() else {
        return !primitive || primitive_element(element);
    };
    let inline = element
        .children()
        .find(|node| xsd_node(*node, "complexType"));
    if let Some(complex) = inline {
        element = complex;
    } else if let Some(name) = element.attribute("type") {
        let Some(complex) = unique_declaration(schema, element, "complexType", name) else {
            return false;
        };
        element = complex;
    } else {
        return false;
    }
    let mut candidates = Vec::new();
    if !required_particle_children(schema, element, name, &mut candidates, depth + 1)
        || candidates.len() != 1
    {
        return false;
    }
    required_element_path(schema, candidates[0], rest, primitive, depth + 1)
}

fn primitive_element(element: roxmltree::Node<'_, '_>) -> bool {
    if element
        .children()
        .any(|node| node.is_element() && !xsd_node(node, "annotation"))
    {
        return false;
    }
    let Some(datatype) = element.attribute("type") else {
        return false;
    };
    let (prefix, local) = datatype
        .split_once(':')
        .map_or((None, datatype), |(prefix, local)| (Some(prefix), local));
    element.lookup_namespace_uri(prefix) == Some("http://www.w3.org/2001/XMLSchema")
        && matches!(local, "string" | "boolean" | "long" | "double")
}

fn required_particle_children<'a, 'input>(
    schema: roxmltree::Node<'a, 'input>,
    container: roxmltree::Node<'a, 'input>,
    name: &str,
    candidates: &mut Vec<roxmltree::Node<'a, 'input>>,
    depth: usize,
) -> bool {
    if depth >= 128 {
        return false;
    }
    for child in container.children().filter(roxmltree::Node::is_element) {
        if xsd_node(child, "element") {
            let declared = child.attribute("name").or_else(|| {
                child
                    .attribute("ref")
                    .and_then(|reference| local_schema_name(schema, child, reference))
            });
            if declared == Some(name) {
                candidates.push(child);
            }
        } else if xsd_node(child, "sequence") || xsd_node(child, "all") {
            let mut nested = Vec::new();
            if !required_particle_children(schema, child, name, &mut nested, depth + 1) {
                return false;
            }
            if !nested.is_empty() && !occurs_once(child) {
                return false;
            }
            candidates.extend(nested);
        } else if !xsd_node(child, "annotation") && !xsd_node(child, "attribute") {
            // Conditional or inherited ownership needs a separate proof.
            return false;
        }
    }
    true
}

fn valid_constant(text: &str, datatype: &str, ty: ScalarType) -> bool {
    match ty {
        ScalarType::String => matches!(datatype, "" | "string" | "anyURI"),
        ScalarType::Bool => datatype == "boolean" && matches!(text, "true" | "false" | "0" | "1"),
        ScalarType::Int => {
            matches!(datatype, "integer" | "int" | "long") && text.parse::<i64>().is_ok()
        }
        ScalarType::Float => {
            matches!(datatype, "decimal" | "double" | "float")
                && text.parse::<f64>().is_ok_and(f64::is_finite)
        }
    }
}
