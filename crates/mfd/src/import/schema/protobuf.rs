use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mapping::{FormatOptions, ProtobufOptions, ProtobufSchemaFile};

use super::{
    ComponentFormat, SchemaComponent, collect_entry_ports, entry_key_sets, entry_tree_schema,
    is_default_output, parse_u32, schema_node_at,
};

#[derive(Clone, Copy)]
enum Direction {
    Source,
    Target,
}

impl Direction {
    const fn label(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Target => "target",
        }
    }
}

/// Imports MapForce binary boundaries backed by a proto2/proto3 schema.
pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    let name = component.attribute("name").unwrap_or_default().to_string();
    if component.attribute("kind") != Some("33") {
        return Err("only kind=33 protobuf binary components are supported".to_string());
    }
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .ok_or_else(|| "component has no data block".to_string())?;
    let root = data
        .children()
        .find(|node| node.has_tag_name("root"))
        .ok_or_else(|| "component has no visible entry tree".to_string())?;
    let documents = root
        .descendants()
        .filter(|node| node.has_tag_name("entry") && node.attribute("type") == Some("doc-protobuf"))
        .collect::<Vec<_>>();
    let [document_entry] = documents.as_slice() else {
        return Err("component must expose exactly one doc-protobuf entry".to_string());
    };
    let document = document_entry
        .children()
        .find(|node| node.has_tag_name("document"))
        .ok_or_else(|| "doc-protobuf entry has no document metadata".to_string())?;
    let payload = document_entry
        .children()
        .find(|node| node.has_tag_name("entry"))
        .ok_or_else(|| "doc-protobuf entry has no message root".to_string())?;
    let binary = data
        .children()
        .find(|node| node.has_tag_name("binary"))
        .ok_or_else(|| "component has no binary instance metadata".to_string())?;

    let (declared_inputs, declared_outputs) = entry_key_sets(&root);
    let direction = match (declared_inputs.is_empty(), declared_outputs.is_empty()) {
        (true, false) if binary.attribute("outputinstance").is_none() => Direction::Source,
        (false, true) if binary.attribute("inputinstance").is_none() => Direction::Target,
        (true, true) => return Err("protobuf component has no input or output ports".to_string()),
        (true, false) | (false, true) | (false, false) => {
            return Err("mixed-direction protobuf components are not supported".to_string());
        }
    };

    let fallback = || entry_tree_schema(&payload);
    let (schema, options) = match load_typed_layout(&document, &payload, mfd_path) {
        Ok(typed) => typed,
        Err(reason) => {
            warnings.push(format!(
                "protobuf component `{name}`: {reason}; imported the visible entry-tree {} shape without executable protobuf metadata",
                direction.label()
            ));
            (fallback(), FormatOptions::default())
        }
    };
    let declared_ports = match direction {
        Direction::Source => &declared_outputs,
        Direction::Target => &declared_inputs,
    };
    let ports = boundary_ports(
        root,
        document_entry,
        &payload,
        BoundaryPortContext {
            declared: declared_ports,
            schema: &schema,
            component_name: &name,
            direction,
        },
        warnings,
    );
    if ports.is_empty() {
        return Err(format!(
            "protobuf {} has no ports matching its message schema",
            direction.label()
        ));
    }
    let is_source = matches!(direction, Direction::Source);
    let input_keys = if is_source {
        BTreeSet::new()
    } else {
        ports.keys().copied().collect()
    };
    let output_keys = if is_source {
        ports.keys().copied().collect()
    } else {
        BTreeSet::new()
    };

    Ok(SchemaComponent {
        name,
        format: ComponentFormat::Protobuf,
        schema,
        input_instance: is_source
            .then(|| binary.attribute("inputinstance").map(str::to_string))
            .flatten(),
        output_instance: (!is_source)
            .then(|| binary.attribute("outputinstance").map(str::to_string))
            .flatten(),
        options,
        is_source,
        is_default_output: !is_source && is_default_output(component),
        is_variable: false,
        is_pass_through: false,
        compute_when_key: None,
        ports,
        input_ancestors: BTreeMap::new(),
        input_keys,
        output_keys,
        db_queries: Vec::new(),
        db_xml_columns: BTreeMap::new(),
        dynamic_json: None,
    })
}

fn load_typed_layout(
    document: &roxmltree::Node<'_, '_>,
    payload: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<(ir::SchemaNode, FormatOptions), String> {
    let schema_file = document
        .attribute("schemafile")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "document metadata has no schemafile".to_string())?;
    let base = mfd_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let bundle = load_schema_bundle(base, schema_file)
        .map_err(|error| format!("could not load protobuf schema `{schema_file}` ({error})"))?;
    let layout = bundle
        .layout()
        .map_err(|error| format!("could not parse protobuf schema `{schema_file}` ({error})"))?;

    let payload_name = payload.attribute("name").unwrap_or_default();
    let declared_root = document.attribute("root").map(normalize_root_name);
    let root = resolve_root(&layout, declared_root.as_deref(), payload_name)?;
    let root_name = layout
        .message(root)
        .map(|message| message.full_name().to_string())
        .ok_or_else(|| "resolved protobuf root is missing from its layout".to_string())?;
    let schema = format_protobuf::to_ir_schema(&layout, &root_name)
        .map_err(|error| format!("protobuf root `{root_name}` is unsupported ({error})"))?;
    let (root_path, schema_text, imports) = bundle.into_parts();
    let has_imports = !imports.is_empty();
    Ok((
        schema,
        FormatOptions {
            protobuf: Some(ProtobufOptions {
                schema: schema_text,
                root_message: root_name,
                schema_path: has_imports.then_some(root_path),
                imports: imports
                    .into_iter()
                    .map(|file| {
                        let (path, source) = file.into_parts();
                        ProtobufSchemaFile { path, source }
                    })
                    .collect(),
            }),
            ..FormatOptions::default()
        },
    ))
}

fn load_schema_bundle(
    base: &Path,
    schema_file: &str,
) -> Result<format_protobuf::SchemaBundle, format_protobuf::ProtobufError> {
    if let Some((directory, root_path)) = schema_file.split_once('/')
        && directory.ends_with("-protobuf")
        && !root_path.is_empty()
    {
        let confined_base = std::fs::canonicalize(base)?;
        let bundle_base = std::fs::canonicalize(base.join(directory))?;
        if !bundle_base.starts_with(&confined_base) {
            return Err(format_protobuf::ProtobufError::InvalidSchema(format!(
                "protobuf bundle directory `{directory}` escapes the mapping directory"
            )));
        }
        return format_protobuf::SchemaBundle::read_relative(&bundle_base, root_path);
    }
    format_protobuf::SchemaBundle::read_relative(base, schema_file)
}

fn resolve_root(
    layout: &format_protobuf::Layout,
    declared: Option<&str>,
    payload_name: &str,
) -> Result<format_protobuf::MessageId, String> {
    if let Some(root) = declared
        && let Ok(message) = layout.resolve_message(root)
    {
        return Ok(message);
    }
    if !payload_name.is_empty()
        && let Ok(message) = layout.resolve_message(payload_name)
    {
        return Ok(message);
    }
    let requested = declared
        .filter(|root| !root.is_empty())
        .or((!payload_name.is_empty()).then_some(payload_name))
        .unwrap_or("<missing>");
    Err(format!(
        "protobuf root `{requested}` does not name a supported message in the schema"
    ))
}

fn normalize_root_name(root: &str) -> String {
    let root = root.trim();
    if let Some((namespace, local)) = root
        .strip_prefix('{')
        .and_then(|expanded| expanded.split_once('}'))
    {
        return if namespace.is_empty() {
            local.to_string()
        } else if local.is_empty() {
            namespace.to_string()
        } else {
            format!("{namespace}.{local}")
        };
    }
    root.to_string()
}

struct BoundaryPortContext<'a> {
    declared: &'a BTreeSet<u32>,
    schema: &'a ir::SchemaNode,
    component_name: &'a str,
    direction: Direction,
}

fn boundary_ports(
    root: roxmltree::Node<'_, '_>,
    document_entry: &roxmltree::Node<'_, '_>,
    payload: &roxmltree::Node<'_, '_>,
    context: BoundaryPortContext<'_>,
    warnings: &mut Vec<String>,
) -> BTreeMap<u32, Vec<String>> {
    let mut ports = BTreeMap::new();
    for wrapper in document_entry
        .ancestors()
        .take_while(|node| *node != root)
        .filter(|node| node.has_tag_name("entry"))
        .chain(std::iter::once(*document_entry))
        .chain(std::iter::once(*payload))
    {
        let key = match context.direction {
            Direction::Source => parse_u32(wrapper.attribute("outkey")),
            Direction::Target => parse_u32(wrapper.attribute("inpkey")),
        };
        if let Some(key) = key.filter(|key| context.declared.contains(key)) {
            ports.insert(key, Vec::new());
        }
    }

    let mut all_payload_ports = BTreeMap::new();
    let mut output_count = 0;
    let mut input_count = 0;
    collect_entry_ports(
        payload,
        &mut Vec::new(),
        &mut all_payload_ports,
        &mut output_count,
        &mut input_count,
    );
    for (key, path) in all_payload_ports {
        if !context.declared.contains(&key) {
            continue;
        }
        if schema_node_at(context.schema, &path).is_some() {
            ports.insert(key, path);
        } else {
            warnings.push(format!(
                "protobuf component `{}` {} port {key} targets unknown schema path `{}`; connection skipped",
                context.component_name,
                context.direction.label(),
                path.join("/")
            ));
        }
    }
    ports
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{normalize_root_name, read};

    #[test]
    fn expands_mapforce_namespace_root_notation() {
        assert_eq!(
            normalize_root_name("{demo.people}Directory"),
            "demo.people.Directory"
        );
        assert_eq!(normalize_root_name("{}Directory"), "Directory");
        assert_eq!(normalize_root_name("Directory"), "Directory");
    }

    #[test]
    fn unreadable_schema_keeps_a_non_executable_entry_tree_target() {
        let document = roxmltree::Document::parse(
            r#"<component name="fallback" library="binary" kind="33">
                <properties XSLTDefaultOutput="1"/>
                <data>
                    <root><entry name="FileInstance"><entry name="document" type="doc-protobuf">
                        <document schemafile="missing.proto" root="Message"/>
                        <entry name="Message"><entry name="value" inpkey="7"/></entry>
                    </entry></entry></root>
                    <binary outputinstance="message.bin"/>
                </data>
            </component>"#,
        )
        .unwrap();
        let mut warnings = Vec::new();
        let component = read(
            &document.root_element(),
            Path::new("/definitely/missing/mapping.mfd"),
            &mut warnings,
        )
        .unwrap();

        assert!(component.options.protobuf.is_none());
        assert!(component.schema.child("value").is_some());
        assert!(warnings.iter().any(|warning| {
            warning.contains("could not load protobuf schema")
                && warning.contains("without executable protobuf metadata")
        }));
    }

    #[test]
    fn unreadable_schema_keeps_a_non_executable_entry_tree_source() {
        let document = roxmltree::Document::parse(
            r#"<component name="fallback" library="binary" kind="33">
                <data>
                    <root><entry name="FileInstance"><entry name="document" type="doc-protobuf">
                        <document schemafile="missing.proto" root="Message"/>
                        <entry name="Message"><entry name="value" outkey="7"/></entry>
                    </entry></entry></root>
                    <binary inputinstance="message.bin"/>
                </data>
            </component>"#,
        )
        .unwrap();
        let mut warnings = Vec::new();
        let component = read(
            &document.root_element(),
            Path::new("/definitely/missing/mapping.mfd"),
            &mut warnings,
        )
        .unwrap();

        assert!(component.is_source);
        assert_eq!(component.input_instance.as_deref(), Some("message.bin"));
        assert!(component.output_keys.contains(&7));
        assert!(component.options.protobuf.is_none());
        assert!(component.schema.child("value").is_some());
        assert!(warnings.iter().any(|warning| {
            warning.contains("visible entry-tree source shape")
                && warning.contains("without executable protobuf metadata")
        }));
    }
}
