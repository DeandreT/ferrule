use std::collections::BTreeMap;
use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::FormatOptions;

use crate::resource::ResourceResolver;

use super::{
    ComponentFormat, JSON_DYNAMIC_BOOL_PORT, JSON_DYNAMIC_FLOAT_PORT, JSON_DYNAMIC_INT_PORT,
    JSON_DYNAMIC_NAME_PORT, JSON_DYNAMIC_STRING_PORT, JsonDynamicPort, PortDeclarationCensus,
    SchemaComponent, entry_key_sets, has_target_node_controls, input_port_ancestry_projected,
    is_default_output, json_undeclared, json_variable, nested_file_instance, parse_u32,
    record_entry_keys, shared, split_json_dynamic_port,
};

/// Reads a JSON component and normalizes structural entry wrappers away.
#[cfg(test)]
pub(in super::super) fn read_json_component(
    component: &roxmltree::Node,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    read_json_component_resolved(component, mfd_path, None, warnings)
}

pub(in super::super) fn read_json_component_in_package(
    component: &roxmltree::Node,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    read_json_component_resolved(
        component,
        resources.mapping_path(),
        Some(resources),
        warnings,
    )
}

fn read_json_component_resolved(
    component: &roxmltree::Node,
    mfd_path: &Path,
    resources: Option<&ResourceResolver>,
    warnings: &mut Vec<String>,
) -> Option<SchemaComponent> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    let data = component
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "data")?;
    let json_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "json");
    let root_el = data
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "root")?;

    // Strip the synthetic FileInstance/document levels down to the JSON
    // document root wrapper (an entry conventionally named `root`).
    let mut entry = root_el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    while matches!(
        entry.attribute("name"),
        Some("FileInstance") | Some("document")
    ) {
        entry = entry
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "entry")?;
    }

    let json_lines = json_el.is_some_and(|json| json.attribute("jsonlines") == Some("1"));
    let input_instance = json_el
        .and_then(|json| json.attribute("inputinstance"))
        .map(str::to_string)
        .or_else(|| nested_file_instance(&root_el, "inputinstance"));
    let output_instance = json_el
        .and_then(|json| json.attribute("outputinstance"))
        .map(str::to_string)
        .or_else(|| nested_file_instance(&root_el, "outputinstance"));
    let json5_setting = json_el.and_then(|json| json.attribute("json5"));
    let json5_extension = [input_instance.as_deref(), output_instance.as_deref()]
        .into_iter()
        .flatten()
        .any(|path| {
            Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json5"))
        });
    let json5_interchange_dependency = json5_setting
        .map(
            |value| mapping::Json5InterchangeDependency::ComponentSetting {
                value: value.into(),
            },
        )
        .or_else(|| {
            json5_extension.then_some(mapping::Json5InterchangeDependency::InstanceExtension {})
        });
    if let Some(dependency) = &json5_interchange_dependency {
        warnings.push(format!(
            "JSON component `{name}` retains its native setting and instance paths for repair: {dependency}"
        ));
    }
    let external_source = json_el.and_then(|json| {
        let metadata = json
            .children()
            .find(|node| node.has_tag_name("ferrule-external-source"))?;
        if metadata.attribute("version") != Some("1") {
            warnings.push(format!(
                "captured user-function JSON source `{name}` has an unsupported provenance metadata version; imported as an ordinary JSON component"
            ));
            return None;
        }
        let source = metadata
            .text()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "metadata is empty".to_string())
            .and_then(|encoded| {
                serde_json::from_str::<mapping::ExternalSourceOptions>(encoded)
                    .map_err(|error| error.to_string())
            });
        match source {
            Ok(source)
                if source.payload() == mapping::ExternalPayloadFormat::Json
                    && matches!(
                        source.origin(),
                        mapping::ExternalSourceOrigin::UserFunction { .. }
                    ) =>
            {
                Some(source)
            }
            Ok(_) => {
                warnings.push(format!(
                    "captured user-function JSON source `{name}` has provenance for a different boundary kind; imported as an ordinary JSON component"
                ));
                None
            }
            Err(error) => {
                warnings.push(format!(
                    "captured user-function JSON source `{name}` has invalid provenance metadata ({error}); imported as an ordinary JSON component"
                ));
                None
            }
        }
    });

    let dynamic_json = match super::dynamic_json::read_target(&entry) {
        Ok(target) => target,
        Err(reason) => {
            warnings.push(format!(
                "dynamic JSON target `{name}` is unsupported: {reason}"
            ));
            None
        }
    };
    let mut ports = BTreeMap::new();
    let mut out_count = 0usize;
    let mut in_count = 0usize;
    let mut declarations = PortDeclarationCensus::default();
    declarations.record(entry, &[]);
    record_entry_keys(&entry, &[], &mut ports, &mut out_count, &mut in_count);
    collect_json_ports_with_declarations(
        &entry,
        &mut Vec::new(),
        &mut ports,
        &mut out_count,
        &mut in_count,
        warnings,
        Some(&mut declarations),
    );
    if let Some(target) = &dynamic_json {
        in_count += target.input_count();
    }
    if out_count == 0 && in_count == 0 && !super::json_parser::has_no_declared_outputs(component) {
        warnings.push(format!("component `{name}` has no connected ports"));
    }
    let is_source = out_count >= in_count;

    // A canonical best-effort export points `schema` at a generated fallback
    // file. Keep the original missing reference separately; runtime I/O never
    // reopens it, and a later import must not mistake the fallback for a full
    // recovery of the original schema.
    let mut json_schema_unresolved_reference = json_el
        .and_then(|json| json.attribute("ferrule-unresolved-json-schema"))
        .map(str::to_string);
    if let Some(reference) = &json_schema_unresolved_reference {
        if reference.is_empty() || reference.len() > 4096 || reference.chars().any(char::is_control)
        {
            warnings.push(format!(
                "component `{name}` has invalid ferrule-unresolved-json-schema metadata; export will reject it until repaired"
            ));
        } else {
            warnings.push(format!(
                "component `{name}` retains unresolved original JSON Schema `{reference}`; the generated schema is an entry-tree fallback"
            ));
        }
    }

    // Schema: prefer the referenced JSON Schema (types + repeating info).
    let mut json_schema_authoritative = false;
    let mut json_schema_imported = false;
    let mut schema = json_el
        .and_then(|j| j.attribute("schema"))
        .and_then(|rel| {
            let imported =
                shared::read_json_schema_resource(mfd_path, resources, rel, "JSON Schema");
            match imported {
                Ok(schema) => {
                    json_schema_imported = true;
                    json_schema_authoritative = ordinary_json_schema_declaration(component);
                    Some(schema)
                }
                Err(e) => {
                    if json_schema_unresolved_reference.is_none() {
                        json_schema_unresolved_reference = Some(rel.to_string());
                    }
                    warnings.push(format!(
                        "component `{name}`: could not read schema `{rel}` ({e}); \
                         falling back to the entry tree"
                    ));
                    None
                }
            }
        })
        .unwrap_or_else(|| {
            if json_el.and_then(|j| j.attribute("schema")).is_none() {
                warnings.push(format!(
                    "component `{name}` has no schema reference; deriving the \
                     schema from the entry tree"
                ));
            }
            json_entry_value_schema(&name, &entry)
        });
    let is_variable = match json_variable::select(data, &schema) {
        Ok(Some(selected)) => {
            schema = selected;
            true
        }
        Ok(None) => false,
        Err(reason) => {
            warnings.push(format!(
                "JSON variable `{name}` has an unsupported root selection: {reason}; skipped"
            ));
            return None;
        }
    };
    if schema.contains_json_array_wrapper()
        && let Err(reason) =
            remap_json_array_wrapper_ports(&entry, &schema, &mut Vec::new(), &mut ports)
    {
        warnings.push(format!(
            "JSON component `{name}` has incompatible nested array entries: {reason}; skipped"
        ));
        return None;
    }
    if is_source && let Err(reason) = attach_json_dynamic_source_schema(&mut schema, &ports) {
        warnings.push(format!(
            "dynamic JSON source `{name}` is unsupported: {reason}"
        ));
    }
    if let Some(target) = &dynamic_json
        && !target.attach_schema(&mut schema)
    {
        warnings.push(format!(
            "dynamic JSON target `{name}` is unsupported: open fields cannot be combined with JSON alternatives"
        ));
        return None;
    }
    if !is_source && !is_variable && json_schema_imported {
        json_undeclared::attach(&entry, &mut schema, &name, warnings);
    }

    let (input_keys, output_keys) = entry_key_sets(&root_el);
    let ancestry = input_port_ancestry_projected(&entry, &input_keys, true);

    Some(SchemaComponent {
        name,
        format: ComponentFormat::Json,
        schema,
        input_instance,
        output_instance,
        options: FormatOptions {
            external_source,
            json_document: true,
            json_schema_unresolved_reference,
            json_lines,
            json5_interchange_dependency,
            ..FormatOptions::default()
        },
        is_source,
        is_default_output: is_default_output(component),
        is_variable,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: ancestry.ancestors,
        input_branch_paths: ancestry.branches,
        ambiguous_branch_inputs: ancestry.ambiguous_inputs,
        has_target_node_controls: has_target_node_controls(component),
        input_keys,
        output_keys,
        json_schema_authoritative,
        port_declarations: Some(declarations.finish(root_el)),
        db_queries: Vec::new(),
        db_xml_columns: BTreeMap::new(),
        dynamic_json,
        target_schema_hints: None,
    })
}

/// Maps JSON property ports to schema paths; structural wrappers are transparent.
fn remap_json_array_wrapper_ports(
    entry: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
) -> Result<(), &'static str> {
    if schema.json_array_wrapper {
        let children = entry
            .children()
            .filter(|node| node.has_tag_name("entry"))
            .collect::<Vec<_>>();
        if entry.attribute("name") == Some("array") {
            let [item] = children.as_slice() else {
                return Err(
                    "marked array entries must contain one `item` role owning an inner `array`",
                );
            };
            let nested = item
                .children()
                .filter(|node| node.has_tag_name("entry"))
                .collect::<Vec<_>>();
            if item.attribute("name") != Some("item")
                || item.attribute("type") != Some("json-item")
                || !matches!(nested.as_slice(), [array] if array.attribute("name") == Some("array"))
            {
                return Err(
                    "marked array entries must contain one `item` role owning an inner `array`",
                );
            }
        } else if !matches!(children.as_slice(), [array] if array.attribute("name") == Some("array"))
        {
            return Err("marked array entries require one declared `array` level");
        }
    }
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        let nested = if child.attribute("type") == Some("json-property") {
            let name = child.attribute("name").unwrap_or_default();
            let Some(nested) = schema.child(name) else {
                continue;
            };
            path.push(name.to_owned());
            Some(nested)
        } else if child.attribute("type") == Some("json-item")
            && child.attribute("name") == Some("item")
            && entry.attribute("name") == Some("array")
            && schema.json_array_wrapper
        {
            let Some(item) = schema.json_array_wrapper_item() else {
                continue;
            };
            path.push(item.name.clone());
            Some(item)
        } else {
            None
        };
        for attribute in ["inpkey", "outkey"] {
            if let Some(key) = parse_u32(child.attribute(attribute))
                && let Some(existing) = ports.get_mut(&key)
            {
                *existing = path.clone();
            }
        }
        remap_json_array_wrapper_ports(&child, nested.unwrap_or(schema), path, ports)?;
        if nested.is_some() {
            path.pop();
        }
    }
    Ok(())
}

pub(in super::super) fn collect_json_ports_with_declarations<'doc, 'input>(
    entry: &roxmltree::Node<'doc, 'input>,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
    in_count: &mut usize,
    warnings: &mut Vec<String>,
    mut declarations: Option<&mut PortDeclarationCensus<'doc, 'input>>,
) {
    for child in entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        match child.attribute("type") {
            Some("json-property") => {
                if record_json_dynamic_source_ports(&child, path, ports, out_count) {
                    continue;
                }
                if child.attribute("inpkey").is_some()
                    && child.children().any(|entry| {
                        entry.has_tag_name("entry")
                            && entry.attribute("type") == Some("json-propertyname")
                            && entry.attribute("inpkey").is_some()
                    })
                {
                    continue;
                }
                path.push(child.attribute("name").unwrap_or_default().to_string());
                if let Some(declarations) = declarations.as_deref_mut() {
                    declarations.record(child, path);
                }
                record_entry_keys(&child, path, ports, out_count, in_count);
                collect_json_ports_with_declarations(
                    &child,
                    path,
                    ports,
                    out_count,
                    in_count,
                    warnings,
                    declarations.as_deref_mut(),
                );
                path.pop();
            }
            Some("json-subtype") => {
                // A subtype is an alternative object view, not a path
                // segment. Its root and descendant ports address the same
                // property path represented by the merged schema projection.
                if let Some(declarations) = declarations.as_deref_mut() {
                    declarations.record(child, path);
                }
                record_entry_keys(&child, path, ports, out_count, in_count);
                collect_json_ports_with_declarations(
                    &child,
                    path,
                    ports,
                    out_count,
                    in_count,
                    warnings,
                    declarations.as_deref_mut(),
                );
            }
            Some("json-propertyname") => {
                if child.attribute("outkey").is_some()
                    || child.attribute("inpkey").is_some()
                    || child
                        .descendants()
                        .any(|d| d.attribute("outkey").is_some() || d.attribute("inpkey").is_some())
                {
                    warnings.push(format!(
                        "dynamic `{}` entries are not supported; connections \
                         under `{}` skipped",
                        child.attribute("type").unwrap_or_default(),
                        path.join("/")
                    ));
                }
            }
            _ => {
                // Ferrule carries explicit JSON null through the nullable
                // typed value rather than a second target port. When both
                // alternatives are connected, keep the typed port and discard
                // the redundant null port so one field receives one binding.
                if child.attribute("name") == Some("null")
                    && child.attribute("inpkey").is_some()
                    && entry.children().any(|sibling| {
                        sibling.has_tag_name("entry")
                            && sibling.attribute("name") != Some("null")
                            && sibling.attribute("inpkey").is_some()
                    })
                {
                    continue;
                }
                if let Some(declarations) = declarations.as_deref_mut() {
                    declarations.record(child, path);
                }
                record_entry_keys(&child, path, ports, out_count, in_count);
                collect_json_ports_with_declarations(
                    &child,
                    path,
                    ports,
                    out_count,
                    in_count,
                    warnings,
                    declarations.as_deref_mut(),
                );
            }
        }
    }
}

fn record_json_dynamic_source_ports(
    property: &roxmltree::Node<'_, '_>,
    owner: &[String],
    ports: &mut BTreeMap<u32, Vec<String>>,
    out_count: &mut usize,
) -> bool {
    let children = property
        .children()
        .filter(|node| node.has_tag_name("entry"))
        .collect::<Vec<_>>();
    let Some(name) = children.iter().find(|node| {
        node.attribute("type") == Some("json-propertyname")
            && parse_u32(node.attribute("outkey")).is_some()
    }) else {
        return false;
    };
    let values = children
        .iter()
        .filter_map(|node| {
            let ty = match node.attribute("name")? {
                "boolean" => ScalarType::Bool,
                "integer" => ScalarType::Int,
                "number" => ScalarType::Float,
                "string" => ScalarType::String,
                _ => return None,
            };
            Some((parse_u32(node.attribute("outkey"))?, ty))
        })
        .collect::<Vec<_>>();
    let [(value_key, value_type)] = values.as_slice() else {
        return false;
    };
    let Some(name_key) = parse_u32(name.attribute("outkey")) else {
        return false;
    };
    let value_marker = match value_type {
        ScalarType::Bool => JSON_DYNAMIC_BOOL_PORT,
        ScalarType::Int => JSON_DYNAMIC_INT_PORT,
        ScalarType::Float => JSON_DYNAMIC_FLOAT_PORT,
        ScalarType::String => JSON_DYNAMIC_STRING_PORT,
    };
    let mut name_path = owner.to_vec();
    name_path.push(JSON_DYNAMIC_NAME_PORT.to_string());
    let mut value_path = owner.to_vec();
    value_path.push(value_marker.to_string());
    ports.insert(name_key, name_path);
    ports.insert(*value_key, value_path);
    *out_count += 2;
    true
}

/// A bounded ordinary JSON reader must not derive authority from the general
/// reader's first-data/root/json normalization or ignored boundary controls.
fn ordinary_json_schema_declaration(component: &roxmltree::Node<'_, '_>) -> bool {
    let data = component
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect::<Vec<_>>();
    let [data] = data.as_slice() else {
        return false;
    };
    let roots = data
        .children()
        .filter(|node| node.has_tag_name("root"))
        .collect::<Vec<_>>();
    let json = data
        .children()
        .filter(|node| node.has_tag_name("json"))
        .collect::<Vec<_>>();
    let ([_], [json]) = (roots.as_slice(), json.as_slice()) else {
        return false;
    };
    data.attributes().len() == 0
        && data.tag_name().namespace().is_none()
        && data
            .children()
            .filter(|node| node.is_element())
            .all(|node| matches!(node.tag_name().name(), "root" | "json"))
        && json
            .attribute("schema")
            .is_some_and(|schema| !schema.is_empty())
        && json.tag_name().namespace().is_none()
        && matches!(json.attribute("jsonlines"), None | Some("0"))
        && json.attributes().all(|attribute| {
            attribute.namespace().is_none()
                && matches!(
                    attribute.name(),
                    "schema" | "inputinstance" | "outputinstance" | "jsonlines"
                )
        })
        && !json.children().any(|node| node.is_element())
        && !component.descendants().any(|node| {
            node.has_tag_name("entry")
                && node
                    .children()
                    .any(|child| child.has_tag_name("outputnodefunctions"))
        })
}

fn attach_json_dynamic_source_schema(
    schema: &mut SchemaNode,
    ports: &BTreeMap<u32, Vec<String>>,
) -> Result<(), String> {
    for path in ports.values() {
        let Some((owner_path, JsonDynamicPort::Value(value_type))) = split_json_dynamic_port(path)
        else {
            continue;
        };
        let owner = schema_node_at_path_mut(schema, owner_path).ok_or_else(|| {
            format!(
                "open object `{}` is absent from its schema",
                owner_path.join("/")
            )
        })?;
        if let Some(existing) = owner.dynamic_fields() {
            if existing.repeating {
                return Err(format!(
                    "open object `{}` declares incompatible dynamic value shapes",
                    owner_path.join("/")
                ));
            }
            if existing.json_any {
                if !owner.set_dynamic_fields(Some(SchemaNode::scalar("*", value_type))) {
                    return Err(format!(
                        "open object `{}` cannot refine its arbitrary dynamic value shape",
                        owner_path.join("/")
                    ));
                }
            } else if !matches!(existing.kind, SchemaKind::Scalar { ty } if ty == value_type) {
                return Err(format!(
                    "open object `{}` declares incompatible dynamic value shapes",
                    owner_path.join("/")
                ));
            }
        } else if !owner.set_dynamic_fields(Some(SchemaNode::scalar("*", value_type))) {
            return Err(format!(
                "open object `{}` cannot combine dynamic fields with schema alternatives",
                owner_path.join("/")
            ));
        }
    }
    Ok(())
}

fn schema_node_at_path_mut<'a>(
    schema: &'a mut SchemaNode,
    path: &[String],
) -> Option<&'a mut SchemaNode> {
    let mut current = schema;
    for segment in path {
        let SchemaKind::Group { children, .. } = &mut current.kind else {
            return None;
        };
        current = children.iter_mut().find(|child| child.name == *segment)?;
    }
    Some(current)
}

/// Fallback JSON schema straight from the entry tree: `json-property`
/// children become fields, an enclosing `array` marks the property
/// repeating, type-leaf names give scalar types.
pub(in super::super) fn json_entry_value_schema(name: &str, entry: &roxmltree::Node) -> SchemaNode {
    for child in entry
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "entry")
    {
        if child.attribute("type").is_some() && child.attribute("type") != Some("json-item") {
            continue;
        }
        match child.attribute("name") {
            Some("object") => {
                let children = child
                    .children()
                    .filter(|n| {
                        n.is_element()
                            && n.tag_name().name() == "entry"
                            && n.attribute("type") == Some("json-property")
                    })
                    .map(|p| json_entry_value_schema(p.attribute("name").unwrap_or_default(), &p))
                    .collect();
                return SchemaNode::group(name, children);
            }
            Some("array") => {
                let mut item = json_entry_value_schema(name, &child);
                if item.repeating {
                    item.name = ir::JSON_ARRAY_ITEM_FIELD.to_owned();
                    let mut wrapper = SchemaNode::group(name, vec![item]).repeating();
                    wrapper.json_array_wrapper = true;
                    return wrapper;
                }
                return item.repeating();
            }
            Some("item") => {
                // An array's item wrapper: its child describes the value.
                return json_entry_value_schema(name, &child);
            }
            Some("number") => return SchemaNode::scalar(name, ir::ScalarType::Float),
            Some("integer") => return SchemaNode::scalar(name, ir::ScalarType::Int),
            Some("boolean") => return SchemaNode::scalar(name, ir::ScalarType::Bool),
            Some("string") | Some("null") => {
                return SchemaNode::scalar(name, ir::ScalarType::String);
            }
            _ => continue,
        }
    }
    SchemaNode::scalar(name, ir::ScalarType::String)
}
