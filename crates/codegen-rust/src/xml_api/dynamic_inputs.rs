//! Additive raw-byte loader entry points for the closed Structured profile.

use crate::rust_string;
use codegen::Program;

pub(super) fn render(program: &Program) -> String {
    let Some((dynamic_index, dynamic)) = program
        .extra_sources
        .iter()
        .enumerate()
        .find(|(_, source)| source.dynamic.is_some())
    else {
        return String::new();
    };
    let static_sources = program
        .extra_sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.dynamic.is_none())
        .collect::<Vec<_>>();
    let dynamic_name = rust_string(&dynamic.name);
    let mut output = String::from("pub use codegen_runtime::DynamicXmlSourceLoader;\n\n");
    for bytes in [false, true] {
        let (stem, dto, source_type, scalar_type, result_type, parser, serializer) = if bytes {
            (
                "execute_xml_bytes",
                "NamedXmlBytesInput",
                "&[u8]",
                "Vec<u8>",
                "XmlBytesExecutionOutputs",
                "parse_structured_xml_bytes",
                "serialize_xml_bytes_outputs",
            )
        } else {
            (
                "execute_xml",
                "NamedXmlInput",
                "&str",
                "String",
                "XmlExecutionOutputs",
                "parse_structured_xml",
                "serialize_xml_outputs",
            )
        };
        for context in [false, true] {
            let (suffix, context_arg, context_call, execute) = if context {
                (
                    "_with_sources_context_and_dynamic_source_loader",
                    ", execution: &ExecutionContext<'_>",
                    ", execution",
                    "execute_outputs_with_sources_context_and_dynamic_source_loader",
                )
            } else {
                (
                    "_with_sources_and_dynamic_source_loader",
                    "",
                    "",
                    "execute_outputs_with_sources_and_dynamic_source_loader",
                )
            };
            let name = format!("{stem}_outputs{suffix}");
            let singular = format!("{stem}{suffix}");
            output.push_str(&format!(r#"pub fn {name}(source: {source_type}, inputs: &[{dto}<'_>]{context_arg},
    loader: &dyn DynamicXmlSourceLoader) -> Result<{result_type}, codegen_runtime::XmlExecutionError> {{
    let _count = codegen_runtime::XmlInputSetBudget::new(inputs.len().saturating_add(1))?;
    let supplied_names: Vec<&str> = inputs.iter().map(|input| input.name).collect();
    let indices = codegen_runtime::xml_input_indices(EXTRA_SOURCE_NAMES, &supplied_names)?;
    let mut sizes = Vec::with_capacity(EXTRA_SOURCE_NAMES.len() + 1);
    sizes.push((codegen_runtime::XmlInputSource::Primary, source.len()));
"#));
            for (static_index, (index, input)) in static_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    sizes.push((codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}, inputs[indices[{static_index}]].document.len()));\n"));
            }
            // With zero static declarations the indices result is intentionally empty.
            output.push_str(&format!("    let _ = &indices;\n    let budget = codegen_runtime::preflight_xml_input_sizes_with_budget(&sizes)?;\n    let parsed = codegen_runtime::{parser}(SOURCE_XML_SCHEMA, source).map_err(|error| codegen_runtime::XmlExecutionError::input(codegen_runtime::XmlInputSource::Primary, error))?;\n"));
            for (static_index, (index, input)) in static_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    let parsed_input_{index} = codegen_runtime::{parser}(EXTRA_XML_INPUT_SCHEMA_{index}, inputs[indices[{static_index}]].document).map_err(|error| codegen_runtime::XmlExecutionError::input(codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}, error))?;\n"));
            }
            output.push_str("    let parsed_inputs: Vec<NamedInput<'_>> = vec![\n");
            for (index, input) in &static_sources {
                output.push_str(&format!(
                    "        NamedInput {{ name: {}, instance: &parsed_input_{index} }},\n",
                    rust_string(&input.name)
                ));
            }
            output.push_str(&format!(r#"    ];
    let adapter = codegen_runtime::XmlDynamicSourceAdapter::new(loader,
        codegen_runtime::XmlDynamicSourcePolicy {{ declaration_index: {dynamic_index}, source: {dynamic_name}, schema: EXTRA_XML_INPUT_SCHEMA_{dynamic_index} }}, budget);
    let mapped = {execute}(&parsed, &parsed_inputs{context_call}, &adapter).map_err(|error| adapter.recover(error))?;
    {serializer}(mapped).map_err(codegen_runtime::XmlExecutionError::from)
}}

pub fn {singular}(source: {source_type}, inputs: &[{dto}<'_>]{context_arg},
    loader: &dyn DynamicXmlSourceLoader) -> Result<{scalar_type}, codegen_runtime::XmlExecutionError> {{
    {name}(source, inputs{context_call}, loader).map(|outputs| outputs.primary)
}}

"#));
        }
    }
    output
}
