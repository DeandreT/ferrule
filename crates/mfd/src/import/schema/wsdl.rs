use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{SchemaKind, SchemaNode};
use mapping::{WsdlMessageOptions, WsdlMessageRole};

use super::{SchemaComponent, read_schema_component, schema_node_at};
use crate::import::function::{FnComponent, is_filter};

/// Imports a WSDL operation message as an XML boundary. The service invocation
/// itself remains outside ferrule: request messages are executable XML sources,
/// while output and fault messages are executable XML targets.
pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<SchemaComponent, String> {
    if component.attribute("kind") != Some("17") {
        return Err("only kind=17 WSDL message components are supported".to_string());
    }
    let data = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .ok_or_else(|| "WSDL message has no data block".to_string())?;
    let metadata = data
        .children()
        .find(|node| node.has_tag_name("wsdl"))
        .ok_or_else(|| "WSDL message has no role metadata".to_string())?;
    let role = metadata.attribute("kind");
    if role.is_some_and(|role| !matches!(role, "output" | "fault")) {
        return Err(format!(
            "WSDL message role `{}` is not supported",
            role.unwrap_or_default()
        ));
    }

    let mut result = read_schema_component(component, mfd_path, warnings)
        .ok_or_else(|| "WSDL message has no entry tree".to_string())?;
    let declared_source = role.is_none();
    if declared_source && !result.input_keys.is_empty() {
        return Err("WSDL request message contains target input ports".to_string());
    }
    if !declared_source && !result.output_keys.is_empty() {
        return Err("WSDL response message contains source output ports".to_string());
    }
    result.is_source = declared_source;
    if declared_source {
        result.input_instance = metadata
            .attribute("previewRequestInstanceFile")
            .map(str::to_string);
    }
    result.options.wsdl = read_contract(component, role, metadata.attribute("faultName"))?;
    Ok(result)
}

fn read_contract(
    component: &roxmltree::Node<'_, '_>,
    role: Option<&str>,
    fault_name: Option<&str>,
) -> Result<Option<WsdlMessageOptions>, String> {
    let properties = component
        .ancestors()
        .filter(|node| node.has_tag_name("component"))
        .find_map(|owner| {
            owner.children().find(|node| {
                node.has_tag_name("properties") && node.attribute("WSDLFile").is_some()
            })
        });
    let Some(properties) = properties else {
        return Ok(None);
    };
    let required = |attribute| {
        properties
            .attribute(attribute)
            .ok_or_else(|| format!("mapping WSDL contract has no `{attribute}` property"))
    };
    let file = required("WSDLFile")?;
    let service = required("WSDLService")?;
    let port = required("WSDLPort")?;
    let operation = required("WSDLOperation")?;
    let message_role = match role {
        None => WsdlMessageRole::Request,
        Some("output") => WsdlMessageRole::Response,
        Some("fault") => WsdlMessageRole::Fault,
        Some(other) => return Err(format!("WSDL message role `{other}` is not supported")),
    };
    WsdlMessageOptions::new(
        file,
        service,
        port,
        operation,
        message_role,
        fault_name.map(str::to_string),
    )
    .map(Some)
    .map_err(|error| error.to_string())
}

/// WSDL entry trees do not include the schema behind a structural message
/// part. When a copy-all edge feeds one from a known XML group, retain that
/// group's complete shape so the mapping remains executable.
pub(super) fn refine_connected_targets(
    components: &mut [SchemaComponent],
    functions: &[FnComponent],
    edge_from: &BTreeMap<u32, u32>,
) {
    let structural_outputs = components
        .iter()
        .flat_map(|component| {
            component.output_keys.iter().filter_map(|key| {
                let path = component.ports.get(key)?;
                let node = schema_node_at(&component.schema, path)?;
                matches!(node.kind, SchemaKind::Group { .. }).then(|| (*key, node.clone()))
            })
        })
        .collect::<BTreeMap<_, _>>();
    let function_by_output = functions
        .iter()
        .flat_map(|function| function.outputs.iter().map(move |key| (*key, function)))
        .collect::<BTreeMap<_, _>>();
    let mut replacements = Vec::new();
    for (component_index, component) in components.iter().enumerate() {
        if component.is_source || component.options.wsdl.is_none() {
            continue;
        }
        for input in &component.input_keys {
            let Some(path) = component.ports.get(input) else {
                continue;
            };
            if !schema_node_at(&component.schema, path)
                .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
            {
                continue;
            }
            let Some(feed) = edge_from.get(input) else {
                continue;
            };
            let Some(source) = resolve_structural_feed(
                *feed,
                &structural_outputs,
                &function_by_output,
                edge_from,
                &mut BTreeSet::new(),
            ) else {
                continue;
            };
            replacements.push((component_index, path.clone(), source.clone()));
        }
    }
    for (component_index, path, mut source) in replacements {
        let Some(target) = schema_node_at_mut(&mut components[component_index].schema, &path)
        else {
            continue;
        };
        source.name.clone_from(&target.name);
        *target = source;
    }
}

fn resolve_structural_feed<'a>(
    feed: u32,
    structural_outputs: &'a BTreeMap<u32, SchemaNode>,
    function_by_output: &BTreeMap<u32, &FnComponent>,
    edge_from: &BTreeMap<u32, u32>,
    active: &mut BTreeSet<u32>,
) -> Option<&'a SchemaNode> {
    if !active.insert(feed) || active.len() > 16 {
        return None;
    }
    if let Some(source) = structural_outputs.get(&feed) {
        return Some(source);
    }
    let function = function_by_output.get(&feed)?;
    if !is_filter(function) {
        return None;
    }
    let input = function.inputs.first().copied().flatten()?;
    let upstream = edge_from.get(&input).copied()?;
    resolve_structural_feed(
        upstream,
        structural_outputs,
        function_by_output,
        edge_from,
        active,
    )
}

fn schema_node_at_mut<'a>(
    mut schema: &'a mut SchemaNode,
    path: &[String],
) -> Option<&'a mut SchemaNode> {
    for segment in path {
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            return None;
        };
        schema = children.iter_mut().find(|child| child.name == *segment)?;
    }
    Some(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_component(xml: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(xml).unwrap_or_else(|error| panic!("invalid test XML: {error}"))
    }

    #[test]
    fn request_message_is_an_xml_source_with_its_preview_path() {
        let document = parse_component(
            r#"<component name="request" library="wsdl" kind="17"><data>
                <root><entry name="FindRecord"><entry name="Criteria" outkey="10"/></entry></root>
                <wsdl previewRequestInstanceFile="request.xml"/>
            </data></component>"#,
        );
        let component = document.root_element();
        let mut warnings = Vec::new();
        let imported = read(&component, Path::new("mapping.mfd"), &mut warnings)
            .unwrap_or_else(|error| panic!("request import failed: {error}"));

        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(imported.is_source);
        assert_eq!(imported.input_instance.as_deref(), Some("request.xml"));
        assert_eq!(imported.ports.get(&10), Some(&vec!["Criteria".to_string()]));
    }

    #[test]
    fn response_and_fault_messages_are_xml_targets() {
        for role in ["output", "fault"] {
            let xml = format!(
                r#"<component name="response" library="wsdl" kind="17"><data>
                    <root><entry name="FindRecordResponse"><entry name="Result" inpkey="20"/></entry></root>
                    <wsdl kind="{role}"/>
                </data></component>"#
            );
            let document = parse_component(&xml);
            let component = document.root_element();
            let mut warnings = Vec::new();
            let imported = read(&component, Path::new("mapping.mfd"), &mut warnings)
                .unwrap_or_else(|error| panic!("response import failed: {error}"));

            assert!(warnings.is_empty(), "{warnings:?}");
            assert!(!imported.is_source);
            assert_eq!(imported.ports.get(&20), Some(&vec!["Result".to_string()]));
        }
    }

    #[test]
    fn rejects_mixed_direction_messages() {
        let document = parse_component(
            r#"<component name="response" library="wsdl" kind="17"><data>
                <root><entry name="Response"><entry name="Result" inpkey="20" outkey="21"/></entry></root>
                <wsdl kind="output"/>
            </data></component>"#,
        );
        let component = document.root_element();
        let mut warnings = Vec::new();
        let Err(error) = read(&component, Path::new("mapping.mfd"), &mut warnings) else {
            panic!("mixed-direction response must fail");
        };
        assert_eq!(error, "WSDL response message contains source output ports");
    }
}
