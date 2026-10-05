//! Static Structured named inputs beside an ordinary or observed primary.
//! Zero-named observed reader entry points remain rendered separately.

use crate::{EmitError, rust_string};
use codegen::Program;

pub(super) fn render(program: &Program) -> Result<String, EmitError> {
    let mut output = String::from(
        "#[derive(Debug, Clone, Copy)]\npub struct NamedXmlInput<'a> { pub name: &'a str, pub document: &'a str }\n#[derive(Debug, Clone, Copy)]\npub struct NamedXmlBytesInput<'a> { pub name: &'a str, pub document: &'a [u8] }\n\n",
    );
    for (index, source) in program.extra_sources.iter().enumerate() {
        let descriptor = codegen::serialize_embedded_schema(
            &source.source,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?;
        output.push_str(&format!(
            "const EXTRA_XML_INPUT_SCHEMA_{index}: &str = {};\n",
            rust_string(&descriptor)
        ));
    }
    let static_sources = program
        .extra_sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.dynamic.is_none())
        .collect::<Vec<_>>();
    for bytes in [false, true] {
        let stem = if bytes {
            "execute_xml_bytes"
        } else {
            "execute_xml"
        };
        let dto = if bytes {
            "NamedXmlBytesInput"
        } else {
            "NamedXmlInput"
        };
        let source_type = if bytes { "&[u8]" } else { "&str" };
        let scalar_type = if bytes { "Vec<u8>" } else { "String" };
        let result_type = if bytes {
            "XmlBytesExecutionOutputs"
        } else {
            "XmlExecutionOutputs"
        };
        let parser = if bytes {
            "parse_structured_xml_bytes"
        } else {
            "parse_structured_xml"
        };
        let observed_primary = program.xml_boundary.as_ref().is_some_and(|policy| {
            policy.input.profile() == Some(codegen::XmlInputProfile::RootView)
        });
        let primary_parser = if observed_primary {
            if bytes {
                "parse_xml_bytes"
            } else {
                "parse_xml"
            }
        } else {
            parser
        };
        let primary_arguments = if observed_primary { ", true, true" } else { "" };
        let serializer = if bytes {
            "serialize_xml_bytes_outputs"
        } else {
            "serialize_xml_outputs"
        };
        for context in [false, true] {
            let suffix = if context { "_and_context" } else { "" };
            let old_suffix = if context { "_with_context" } else { "" };
            let context_arg = if context {
                ", execution: &ExecutionContext<'_>"
            } else {
                ""
            };
            let context_call = if context { ", execution" } else { "" };
            let name = format!("{stem}_outputs_with_sources{suffix}");
            let singular = format!("{stem}_with_sources{suffix}");
            let execute = if context {
                "execute_outputs_with_sources_and_context"
            } else {
                "execute_outputs_with_sources"
            };
            output.push_str(&format!("pub fn {name}(source: {source_type}, inputs: &[{dto}<'_>]{context_arg}) -> Result<{result_type}, codegen_runtime::XmlExecutionError> {{\n    let _count = codegen_runtime::XmlInputSetBudget::new(inputs.len().saturating_add(1))?;\n    let supplied_names: Vec<&str> = inputs.iter().map(|input| input.name).collect();\n    let _indices = codegen_runtime::xml_input_indices(EXTRA_SOURCE_NAMES, &supplied_names)?;\n    let mut sizes = Vec::with_capacity(EXTRA_SOURCE_NAMES.len() + 1);\n    sizes.push((codegen_runtime::XmlInputSource::Primary, source.len()));\n"));
            for (static_index, (index, input)) in static_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    sizes.push((codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}, inputs[_indices[{static_index}]].document.len()));\n"));
            }
            output.push_str(&format!("    codegen_runtime::preflight_xml_input_sizes(&sizes)?;\n    let parsed = codegen_runtime::{primary_parser}(SOURCE_XML_SCHEMA, source{primary_arguments}).map_err(|error| codegen_runtime::XmlExecutionError::input(codegen_runtime::XmlInputSource::Primary, error))?;\n"));
            for (static_index, (index, input)) in static_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    let parsed_input_{index} = codegen_runtime::{parser}(EXTRA_XML_INPUT_SCHEMA_{index}, inputs[_indices[{static_index}]].document).map_err(|error| codegen_runtime::XmlExecutionError::input(codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}, error))?;\n"));
            }
            output.push_str("    let parsed_inputs: Vec<NamedInput<'_>> = vec![\n");
            for (index, input) in &static_sources {
                output.push_str(&format!(
                    "        NamedInput {{ name: {}, instance: &parsed_input_{index} }},\n",
                    rust_string(&input.name)
                ));
            }
            output.push_str(&format!("    ];\n    let mapped = {execute}(&parsed, &parsed_inputs{context_call}).map_err(codegen_runtime::XmlBoundaryError::from).map_err(codegen_runtime::XmlExecutionError::from)?;\n    {serializer}(mapped).map_err(codegen_runtime::XmlExecutionError::from)\n}}\n\npub fn {singular}(source: {source_type}, inputs: &[{dto}<'_>]{context_arg}) -> Result<{scalar_type}, codegen_runtime::XmlExecutionError> {{\n    {name}(source, inputs{context_call}).map(|outputs| outputs.primary)\n}}\n\n"));
            // The legacy signatures and inner boundary identities stay unchanged.
            output.push_str(&format!("pub fn {stem}_outputs{old_suffix}(source: {source_type}{context_arg}) -> Result<{result_type}, codegen_runtime::XmlOutputSetError> {{\n    {name}(source, &[]{context_call}).map_err(codegen_runtime::XmlExecutionError::into_output_set)\n}}\n\npub fn {stem}{old_suffix}(source: {source_type}{context_arg}) -> Result<{scalar_type}, codegen_runtime::XmlBoundaryError> {{\n    {stem}_outputs{old_suffix}(source{context_call}).map(|outputs| outputs.primary).map_err(codegen_runtime::XmlOutputSetError::into_boundary)\n}}\n\n"));
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    fn project() -> ::mapping::Project {
        serde_json::from_str(include_str!(
            "../../../codegen/src/tests/fixtures/root_view_static_named_xml_inputs.json"
        ))
        .unwrap()
    }
    #[test]
    fn observed_primary_and_structured_named_parsers_keep_whole_input_phase_order() {
        let program = codegen::lower(&project()).unwrap();
        let output = super::super::render(&program).unwrap();
        assert_eq!(
            output
                .matches("parse_xml(SOURCE_XML_SCHEMA, source, true, true)")
                .count(),
            2
        );
        assert_eq!(
            output
                .matches("parse_xml_bytes(SOURCE_XML_SCHEMA, source, true, true)")
                .count(),
            2
        );
        assert!(output.contains("parse_structured_xml(EXTRA_XML_INPUT_SCHEMA_0"));
        assert!(output.contains("parse_structured_xml_bytes(EXTRA_XML_INPUT_SCHEMA_1"));
        let positions = [
            "XmlInputSetBudget::new(inputs.len().saturating_add(1))",
            "xml_input_indices",
            "preflight_xml_input_sizes(&sizes)",
            "parse_xml(SOURCE_XML_SCHEMA",
            "parse_structured_xml(EXTRA_XML_INPUT_SCHEMA_0",
            "parse_structured_xml(EXTRA_XML_INPUT_SCHEMA_1",
            "let mapped =",
            "serialize_xml_outputs(mapped)",
        ]
        .map(|needle| output.find(needle).unwrap());
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(output.contains("XmlInputSource::Named { index: 1, name: \"labels\" }"));
        let functions = output
            .lines()
            .filter_map(|line| line.strip_prefix("pub fn "))
            .map(|line| line.split('(').next().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(functions.len(), 16);
        for name in [
            "execute_xml_outputs_with_sources",
            "execute_xml_outputs_with_sources_and_context",
            "execute_xml_bytes_outputs_with_sources",
            "execute_xml_bytes_outputs_with_sources_and_context",
            "execute_xml_with_sources",
            "execute_xml_with_sources_and_context",
            "execute_xml_bytes_with_sources",
            "execute_xml_bytes_with_sources_and_context",
        ] {
            assert!(functions.contains(name), "missing public method {name}");
        }
        let mut invalid = program;
        invalid.xml_boundary.as_mut().unwrap().extra_inputs[1].name = "wrong".into();
        assert!(super::super::render(&invalid).is_err());
    }
    #[test]
    fn zero_named_observed_route_keeps_its_existing_methods_and_reader() {
        let project: ::mapping::Project = serde_json::from_str(include_str!(
            "../../../codegen/src/tests/fixtures/static_named_xml_rootview.json"
        ))
        .unwrap();
        let output = super::super::render(&codegen::lower(&project).unwrap()).unwrap();
        assert!(!output.contains("NamedXmlInput"));
        assert!(!output.contains("with_sources"));
        assert!(output.contains("parse_xml(SOURCE_XML_SCHEMA, source, true, true)"));
        assert!(output.contains(
            "pub fn execute_xml(source: &str) -> Result<String, codegen_runtime::XmlBoundaryError>"
        ));
        assert!(output.contains("pub fn execute_xml_bytes_with_context(source: &[u8], execution: &ExecutionContext<'_>) -> Result<Vec<u8>, codegen_runtime::XmlBoundaryError>"));
    }
}
