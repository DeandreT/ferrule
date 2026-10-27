//! Source-only admission for the explicit ordered JSON document boundary.
//! This does not admit a target mapping or turn repair output into executable code.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{JsonSourceProperty, JsonSourceShape};

use super::{ImportOptions, function, graph, schema};
use crate::MfdError;

pub struct ImportedJsonSourceProjection {
    pub shape: JsonSourceShape,
    /// The unchanged loaded schema, including annotations not enforced by selection.
    pub source_schema: SchemaNode,
    pub filename_parameter: String,
    pub filename_preview: Option<String>,
    pub filename_input_port: u32,
    /// Present only when the raw source exposes its filename as an output.
    /// An unused filename output is not synthesized during import.
    pub filename_output_port: Option<u32>,
    /// Exact execution-view collections for the original typed output ports.
    pub ports: BTreeMap<u32, Vec<String>>,
    pub mapping_path: PathBuf,
}

/// Typed selection from an explicit JSON String parameter, never a file source.
/// This source-only artifact does not admit an outer invocation or a mapping body.
pub struct ImportedJsonParameterProjection {
    pub shape: JsonSourceShape,
    pub source_schema: SchemaNode,
    pub parameter_name: String,
    pub parameter_preview: Option<String>,
    pub input_port: u32,
    pub ports: BTreeMap<u32, Vec<String>>,
    pub mapping_path: PathBuf,
}

fn refuse(message: &str) -> MfdError {
    MfdError::UnsupportedImport(format!("ordered JSON source: {message}"))
}

pub fn import_json_source_projection_with_options(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedJsonSourceProjection, MfdError> {
    import_projection(path, options, false)
}

pub fn import_json_parameter_projection_with_options(
    path: &Path,
    options: &ImportOptions,
) -> Result<ImportedJsonParameterProjection, MfdError> {
    let projection = import_projection(path, options, true)?;
    Ok(ImportedJsonParameterProjection {
        shape: projection.shape,
        source_schema: projection.source_schema,
        parameter_name: projection.filename_parameter,
        parameter_preview: projection.filename_preview,
        input_port: projection.filename_input_port,
        ports: projection.ports,
        mapping_path: projection.mapping_path,
    })
}

fn import_projection(
    path: &Path,
    options: &ImportOptions,
    string_parameter: bool,
) -> Result<ImportedJsonSourceProjection, MfdError> {
    let resources = super::resolved_resources(path, options)?;
    let result = (|| {
        let text = resources.read_design()?;
        let document = roxmltree::Document::parse(&text)?;
        let wrapper = document
            .root_element()
            .children()
            .find(|n| n.has_tag_name("component"))
            .ok_or_else(|| refuse("missing mapping wrapper"))?;
        let structure = wrapper
            .children()
            .find(|n| n.has_tag_name("structure"))
            .ok_or_else(|| refuse("missing mapping structure"))?;
        let children = structure
            .children()
            .find(|n| n.has_tag_name("children"))
            .ok_or_else(|| refuse("missing components"))?;
        let edges = graph::read_edge_inputs(&structure, Some(&wrapper));
        if !edges.complete || !edges.conflicts.is_empty() {
            return Err(refuse("incomplete or conflicting connections"));
        }
        let sources: Vec<_> = children
            .children()
            .filter(|n| {
                n.has_tag_name("component")
                    && n.attribute("library") == Some("json")
                    && (!string_parameter
                        || n.children()
                            .filter(|child| child.has_tag_name("data"))
                            .any(|data| {
                                data.children().any(|parameter| {
                                    parameter.has_tag_name("parameter")
                                        && parameter.attribute("usageKind") == Some("stringparse")
                                })
                            }))
                    && n.descendants().any(|entry| {
                        entry.has_tag_name("entry")
                            && entry.attribute("name") == Some("FileInstance")
                            && entry.attribute("inpkey").is_some()
                            && entry.descendants().any(|child| {
                                child.has_tag_name("entry") && child.attribute("outkey").is_some()
                            })
                    })
            })
            .collect();
        let [component] = sources.as_slice() else {
            return Err(refuse(if string_parameter {
                "requires one explicit String-parsed JSON source"
            } else {
                "requires one filename-bearing JSON source"
            }));
        };
        if component.attribute("kind") != Some("31") {
            return Err(refuse("unsupported source kind"));
        }
        if string_parameter
            && ["data", "properties"].iter().any(|name| {
                component
                    .children()
                    .filter(|node| node.has_tag_name(*name))
                    .count()
                    != 1
            })
        {
            return Err(refuse("ambiguous JSON String parameter metadata"));
        }
        let properties = component
            .children()
            .find(|n| n.has_tag_name("properties"))
            .ok_or_else(|| refuse("missing source properties"))?;
        if (!string_parameter && properties.attribute("DynamicFilenames") != Some("1"))
            || (string_parameter && properties.attribute("DynamicFilenames").is_some())
            || properties
                .attributes()
                .any(|a| !matches!(a.name(), "DynamicFilenames" | "XSLTTargetEncoding"))
            || properties
                .attribute("XSLTTargetEncoding")
                .is_some_and(|v| v != "UTF-8")
        {
            return Err(refuse("unsupported source properties"));
        }
        let data = component
            .children()
            .find(|n| n.has_tag_name("data"))
            .ok_or_else(|| refuse("missing source data"))?;
        if string_parameter {
            if data.attributes().len() != 0
                || data
                    .children()
                    .filter(|node| node.is_element())
                    .any(|node| !matches!(node.tag_name().name(), "root" | "parameter" | "json"))
                || ["root", "json"].iter().any(|name| {
                    data.children()
                        .filter(|node| node.has_tag_name(*name))
                        .count()
                        != 1
                })
                || properties.children().any(|node| node.is_element())
            {
                return Err(refuse("unqualified JSON String parameter metadata"));
            }
            let parameters: Vec<_> = data
                .children()
                .filter(|node| node.has_tag_name("parameter"))
                .collect();
            if !matches!(parameters.as_slice(), [parameter]
                if parameter.attribute("usageKind") == Some("stringparse")
                    && parameter.attributes().len() == 1
                    && !parameter.children().any(|child| child.is_element()))
            {
                return Err(refuse("unqualified JSON String parameter declaration"));
            }
        }
        let json = data
            .children()
            .find(|n| n.has_tag_name("json"))
            .ok_or_else(|| refuse("missing JSON settings"))?;
        if json
            .attributes()
            .any(|a| !matches!(a.name(), "schema" | "json5" | "jsonlines"))
            || json.attribute("schema").is_none()
            || json.attribute("json5").is_some_and(|value| value != "0")
            || json
                .attribute("jsonlines")
                .is_some_and(|value| value != "0")
            || json.children().any(|n| n.is_element())
        {
            return Err(refuse("requires an ordinary strict JSON schema source"));
        }
        let files: Vec<_> = data
            .descendants()
            .filter(|n| n.has_tag_name("entry") && n.attribute("name") == Some("FileInstance"))
            .collect();
        let [file] = files.as_slice() else {
            return Err(refuse("ambiguous source file wrapper"));
        };
        let key = |name| {
            file.attribute(name)
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| refuse("invalid filename port"))
        };
        let filename_input_port = key("inpkey")?;
        let filename_output_port = file
            .attribute("outkey")
            .map(|value| {
                value
                    .parse::<u32>()
                    .map_err(|_| refuse("invalid filename port"))
            })
            .transpose()?;
        if string_parameter && filename_output_port.is_some() {
            return Err(refuse(
                "a JSON String parameter has no physical filename output",
            ));
        }
        let parameter_port = edges
            .feeds
            .get(&filename_input_port)
            .ok_or_else(|| refuse("unconnected input filename"))?;
        let parameters: Vec<_> = children
            .children()
            .filter(|n| n.has_tag_name("component"))
            .map(|node| (node, function::read(&node)))
            .filter(|(_, f)| f.output_pins.contains(&Some(*parameter_port)))
            .collect();
        let [(parameter_node, parameter)] = parameters.as_slice() else {
            return Err(refuse("ambiguous input filename expression"));
        };
        let filename_parameter = parameter
            .input_parameter_name
            .as_ref()
            .filter(|p| !p.optional)
            .ok_or_else(|| refuse("filename must be a required global parameter"))?;
        if parameter.kind != 6
            || parameter.library != "core"
            || parameter.inputs != [None]
            || parameter.input_type != Some(ScalarType::String)
            || !parameter.raw_single_output_valid
        {
            return Err(refuse("filename must be a direct String parameter"));
        }
        let input_pins: Vec<_> = parameter_node
            .children()
            .filter(|n| n.has_tag_name("sources"))
            .flat_map(|n| n.children().filter(|n| n.has_tag_name("datapoint")))
            .collect();
        if !matches!(input_pins.as_slice(), [pin] if pin.attributes().len() == 0)
            || filename_parameter.name.is_empty()
            || filename_parameter.name.contains('\0')
            || filename_parameter.name.len() > mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES
        {
            return Err(refuse("unqualified filename parameter declaration"));
        }
        let mut warnings = Vec::new();
        let source = schema::read_json_component_in_package(component, &resources, &mut warnings)
            .ok_or_else(|| refuse("JSON schema did not load"))?;
        if !warnings.is_empty() {
            return Err(refuse("unresolved source schema diagnostics"));
        }
        if !source.is_source || source.is_variable {
            return Err(refuse("requires a physical JSON source"));
        }
        let typed = file
            .descendants()
            .find(|n| n.has_tag_name("entry") && n.attribute("name") == Some("root"))
            .and_then(|root| root.children().find(|n| n.has_tag_name("entry")))
            .ok_or_else(|| refuse("missing typed root"))?;
        let mut ports = BTreeMap::new();
        if typed
            .descendants()
            .filter(|node| node.has_tag_name("entry"))
            .any(|node| {
                node.attributes().any(|attribute| {
                    !matches!(attribute.name(), "name" | "type" | "outkey" | "expanded")
                })
            })
        {
            return Err(refuse("unqualified source entry modifiers"));
        }
        let shape = read_shape(
            typed,
            &source.schema,
            vec!["Documents".into(), "Root".into()],
            &mut ports,
            0,
        )?;
        let declared = typed
            .descendants()
            .filter_map(|node| node.attribute("outkey"))
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| refuse("invalid source output identity"))?;
        let unique = declared
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>();
        if unique.len() != declared.len() || unique != ports.keys().copied().collect() {
            return Err(refuse("unsupported or duplicate source output port"));
        }
        if filename_output_port.is_some_and(|port| ports.contains_key(&port)) {
            return Err(refuse("filename/data port collision"));
        }
        shape
            .validate()
            .map_err(|_| refuse("projection shape exceeds limits"))?;
        Ok(ImportedJsonSourceProjection {
            shape,
            source_schema: source.schema,
            filename_parameter: filename_parameter.name.clone(),
            filename_preview: parameter.input_preview.clone(),
            filename_input_port,
            filename_output_port,
            ports,
            mapping_path: resources.mapping_path().to_path_buf(),
        })
    })();
    resources.finish_bindings()?;
    result
}

fn read_shape(
    entry: roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    path: Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
    depth: usize,
) -> Result<JsonSourceShape, MfdError> {
    if depth > mapping::MAX_JSON_PROJECTION_DEPTH {
        return Err(refuse("source is too deep"));
    }
    if let Some(key) = entry.attribute("outkey") {
        let key = key
            .parse()
            .map_err(|_| refuse("invalid typed output port"))?;
        if ports.insert(key, path.clone()).is_some() {
            return Err(refuse("duplicate typed output port"));
        }
    }
    if entry.attribute("inpkey").is_some() {
        return Err(refuse("source type has an input connection"));
    }
    match entry.attribute("name") {
        Some("object") if matches!(schema.kind, SchemaKind::Group { .. }) => {
            let mut properties = Vec::new();
            for property in entry.children().filter(|n| n.has_tag_name("entry")) {
                if !property
                    .descendants()
                    .any(|n| n.attribute("outkey").is_some())
                {
                    continue;
                }
                if property.attribute("type") != Some("json-property") {
                    return Err(refuse("unsupported object entry"));
                }
                let name = property
                    .attribute("name")
                    .ok_or_else(|| refuse("unnamed property"))?;
                let typed: Vec<_> = property
                    .children()
                    .filter(|n| n.has_tag_name("entry"))
                    .collect();
                let [typed] = typed.as_slice() else {
                    return Err(refuse("ambiguous property type"));
                };
                let child = schema
                    .child(name)
                    .ok_or_else(|| refuse("property absent from original schema"))?;
                let mut child_path = path.clone();
                child_path.push(name.into());
                properties.push(JsonSourceProperty {
                    name: name.into(),
                    shape: read_shape(*typed, child, child_path, ports, depth + 1)?,
                });
            }
            Ok(JsonSourceShape::Object { properties })
        }
        Some("array") if schema.repeating => {
            let items: Vec<_> = entry
                .children()
                .filter(|n| n.has_tag_name("entry") && n.attribute("type") == Some("json-item"))
                .collect();
            let [item] = items.as_slice() else {
                return Err(refuse("ambiguous array item"));
            };
            let typed: Vec<_> = item
                .children()
                .filter(|n| n.has_tag_name("entry"))
                .collect();
            let [typed] = typed.as_slice() else {
                return Err(refuse("ambiguous array item type"));
            };
            let mut child_path = path;
            child_path.push("Items".into());
            let mut child = schema.clone();
            child.repeating = false;
            Ok(JsonSourceShape::Array {
                item: Box::new(read_shape(*typed, &child, child_path, ports, depth + 1)?),
            })
        }
        Some("string") if accepts_scalar(schema, ScalarType::String) => Ok(JsonSourceShape::String),
        Some("number")
            if accepts_scalar(schema, ScalarType::Int)
                || accepts_scalar(schema, ScalarType::Float) =>
        {
            Ok(JsonSourceShape::Number)
        }
        Some("boolean") if accepts_scalar(schema, ScalarType::Bool) => Ok(JsonSourceShape::Boolean),
        _ => Err(refuse("typed entry differs from its original schema")),
    }
}

fn accepts_scalar(schema: &SchemaNode, selected: ScalarType) -> bool {
    // A saved typed connector selects one alternative, not the whole union.
    // The authoritative source schema stays intact on the imported result.
    match schema.kind {
        SchemaKind::Scalar { ty } => ty == selected,
        SchemaKind::ScalarUnion { types } => types.contains(selected),
        SchemaKind::Group { .. } => false,
    }
}
