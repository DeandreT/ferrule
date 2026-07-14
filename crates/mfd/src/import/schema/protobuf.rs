use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mapping::{FormatOptions, ProtobufOptions};

use super::{
    ComponentFormat, SchemaComponent, collect_entry_ports, entry_key_sets, entry_tree_schema,
    is_default_output, parse_u32, schema_node_at,
};

/// Imports target-side MapForce binary components backed by a proto2 schema.
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
    if declared_inputs.is_empty() {
        return Err("protobuf target has no input ports".to_string());
    }
    if !declared_outputs.is_empty() || binary.attribute("inputinstance").is_some() {
        return Err(
            "protobuf source or mixed-direction components are not supported yet".to_string(),
        );
    }

    let fallback = || entry_tree_schema(&payload);
    let (schema, options) = match load_typed_layout(&document, &payload, mfd_path) {
        Ok(typed) => typed,
        Err(reason) => {
            warnings.push(format!(
                "protobuf component `{name}`: {reason}; imported the visible entry-tree target shape without executable protobuf metadata"
            ));
            (fallback(), FormatOptions::default())
        }
    };
    let ports = input_ports(
        root,
        document_entry,
        &payload,
        &declared_inputs,
        &schema,
        &name,
        warnings,
    );
    if ports.is_empty() {
        return Err("protobuf target has no input ports matching its message schema".to_string());
    }
    let input_keys = ports.keys().copied().collect();

    Ok(SchemaComponent {
        name,
        format: ComponentFormat::Protobuf,
        schema,
        input_instance: None,
        output_instance: binary.attribute("outputinstance").map(str::to_string),
        options,
        is_source: false,
        is_default_output: is_default_output(component),
        is_variable: false,
        compute_when_key: None,
        ports,
        input_keys,
        output_keys: BTreeSet::new(),
        db_queries: Vec::new(),
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
    let schema_path = resolve_sibling(mfd_path, schema_file);
    let schema_bytes = std::fs::read(&schema_path)
        .map_err(|error| format!("could not read protobuf schema `{schema_file}` ({error})"))?;
    if schema_bytes.len() > format_protobuf::MAX_SCHEMA_BYTES {
        return Err(format!(
            "protobuf schema `{schema_file}` exceeds the {}-byte limit",
            format_protobuf::MAX_SCHEMA_BYTES
        ));
    }
    let schema_text = String::from_utf8(schema_bytes)
        .map_err(|_| format!("protobuf schema `{schema_file}` is not valid UTF-8"))?;
    let layout = format_protobuf::Layout::parse(&schema_text)
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
    Ok((
        schema,
        FormatOptions {
            protobuf: Some(ProtobufOptions {
                schema: schema_text,
                root_message: root_name,
            }),
            ..FormatOptions::default()
        },
    ))
}

fn resolve_sibling(mfd_path: &Path, relative: &str) -> PathBuf {
    mfd_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(relative)
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

fn input_ports(
    root: roxmltree::Node<'_, '_>,
    document_entry: &roxmltree::Node<'_, '_>,
    payload: &roxmltree::Node<'_, '_>,
    declared_inputs: &BTreeSet<u32>,
    schema: &ir::SchemaNode,
    component_name: &str,
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
        if let Some(key) = parse_u32(wrapper.attribute("inpkey")) {
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
        if !declared_inputs.contains(&key) {
            continue;
        }
        if schema_node_at(schema, &path).is_some() {
            ports.insert(key, path);
        } else {
            warnings.push(format!(
                "protobuf component `{component_name}` input port {key} targets unknown schema path `{}`; connection skipped",
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
            warning.contains("could not read protobuf schema")
                && warning.contains("without executable protobuf metadata")
        }));
    }
}
