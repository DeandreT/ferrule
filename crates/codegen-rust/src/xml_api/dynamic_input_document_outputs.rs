//! One dynamic input, static admission and complete mixed XML serialization.
use super::output_arguments;
use crate::{EmitError, rust_string};
use codegen::{Program, ProgramValidationError};

pub(super) fn render(program: &Program) -> Result<String, EmitError> {
    if program.xml_output_mode()?
        != Some(codegen::XmlOutputMode::DynamicNamedInputStaticPrimaryDynamicNamedDocuments)
    {
        return Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "dynamic input mixed XML outputs require their checked adapter mode".into(),
        }
        .into());
    }
    let policy = program.xml_boundary.as_ref().ok_or_else(|| {
        ProgramValidationError::InvalidXmlBoundary {
            reason: "XML document outputs require a boundary".into(),
        }
    })?;
    if policy.extra_outputs.len() != program.extra_targets.len()
        || program
            .extra_targets
            .iter()
            .zip(&policy.extra_outputs)
            .any(|(target, output)| target.name != output.name)
    {
        return Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "XML document outputs require policies in exact declaration order".into(),
        }
        .into());
    }
    let static_sources = program
        .extra_sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.dynamic.is_none())
        .collect::<Vec<_>>();
    let (dynamic_index, dynamic_source) = program
        .extra_sources
        .iter()
        .enumerate()
        .find(|(_, source)| source.dynamic.is_some())
        .ok_or_else(|| ProgramValidationError::InvalidXmlBoundary {
            reason: "dynamic input mixed XML outputs require their dynamic declaration".into(),
        })?;
    let source = codegen::serialize_embedded_schema(
        &program.source,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let primary = codegen::serialize_embedded_schema(
        &program.target,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let mut output = format!(
        "\nconst SOURCE_XML_SCHEMA: &str = {};\nconst TARGET_XML_SCHEMA: &str = {};\n",
        rust_string(&source),
        rust_string(&primary),
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
    for (declaration_index, target) in program.extra_targets.iter().enumerate() {
        let descriptor = codegen::serialize_embedded_schema(
            &target.target,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?;
        output.push_str(&format!(
            "const NAMED_XML_DOCUMENT_SCHEMA_{declaration_index}: &str = {};\nconst NAMED_XML_DOCUMENT_OUTPUT_NAME_{declaration_index}: &str = {};\n",
            rust_string(&descriptor), rust_string(&target.name),
        ));
    }
    output.push_str("pub use codegen_runtime::DynamicXmlSourceLoader;\n\n");
    output.push_str("\n#[derive(Debug, Clone, Copy)]\npub struct NamedXmlInput<'a> { pub name: &'a str, pub document: &'a str }\n#[derive(Debug, Clone, Copy)]\npub struct NamedXmlBytesInput<'a> { pub name: &'a str, pub document: &'a [u8] }\n\n");
    output.push_str("\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlDocumentOutput { pub path: String, pub document: String }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlBytesDocumentOutput { pub path: String, pub document: Vec<u8> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct NamedXmlDocumentOutputs { pub name: &'static str, pub documents: Vec<XmlDocumentOutput> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct NamedXmlBytesDocumentOutputs { pub name: &'static str, pub documents: Vec<XmlBytesDocumentOutput> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlDocumentExecutionOutputs { pub primary: String, pub extras: Vec<NamedXmlDocumentOutputs> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlBytesDocumentExecutionOutputs { pub primary: Vec<u8>, pub extras: Vec<NamedXmlBytesDocumentOutputs> }\n\n");
    output.push_str(r#"// Only the trusted static admission helpers reach this private conversion.
fn xml_dynamic_document_outputs_input_error(error: codegen_runtime::XmlExecutionError) -> codegen_runtime::XmlDynamicInputDocumentOutputsExecutionError {
    debug_assert!(error.output.is_none() && error.request.is_none());
    codegen_runtime::XmlDynamicInputDocumentOutputsExecutionError::from_input_boundary(error.input, error.boundary)
}

// Only synchronous recovery from this execution's fresh terminal adapter reaches
// this conversion. Matching external host text cannot supply an original Request.
fn xml_dynamic_document_outputs_recovery_error(error: codegen_runtime::XmlExecutionError) -> codegen_runtime::XmlDynamicInputDocumentOutputsExecutionError {
    debug_assert!(error.output.is_none());
    match error.request {
        Some(request) => {
            debug_assert_eq!(error.input, Some(codegen_runtime::XmlInputSource::Named {
                index: request.declaration_index, name: request.source,
            }));
            codegen_runtime::XmlDynamicInputDocumentOutputsExecutionError::from_dynamic_input_boundary(request, error.boundary)
        }
        None => {
            debug_assert!(error.input.is_none());
            codegen_runtime::XmlDynamicInputDocumentOutputsExecutionError::from_input_boundary(None, error.boundary)
        }
    }
}

"#);
    let primary_arguments = output_arguments(&policy.output)?;
    for bytes in [false, true] {
        let stem = if bytes {
            "execute_xml_bytes_document_outputs_with_sources"
        } else {
            "execute_xml_document_outputs_with_sources"
        };
        let input_dto = if bytes {
            "NamedXmlBytesInput"
        } else {
            "NamedXmlInput"
        };
        let source_type = if bytes { "&[u8]" } else { "&str" };
        let parser = if bytes {
            "parse_structured_xml_bytes"
        } else {
            "parse_structured_xml"
        };
        let dto = if bytes {
            "XmlBytesDocumentOutput"
        } else {
            "XmlDocumentOutput"
        };
        let named_dto = if bytes {
            "NamedXmlBytesDocumentOutputs"
        } else {
            "NamedXmlDocumentOutputs"
        };
        let result = if bytes {
            "XmlBytesDocumentExecutionOutputs"
        } else {
            "XmlDocumentExecutionOutputs"
        };
        let helper = if bytes {
            "serialize_xml_bytes_document_outputs"
        } else {
            "serialize_xml_document_outputs"
        };
        let conversion = if bytes { "xml.into_bytes()" } else { "xml" };
        let primary_conversion = if bytes {
            "primary_xml.into_bytes()"
        } else {
            "primary_xml"
        };
        for context in [false, true] {
            let suffix = if context {
                "_context_and_dynamic_source_loader"
            } else {
                "_and_dynamic_source_loader"
            };
            let context_arg = if context {
                ", execution: &codegen_runtime::ExecutionContext<'_>"
            } else {
                ""
            };
            let context_call = if context { ", execution" } else { "" };
            let execute = if context {
                "execute_outputs_with_sources_context_and_dynamic_source_loader"
            } else {
                "execute_outputs_with_sources_and_dynamic_source_loader"
            };
            output.push_str(&format!("pub fn {stem}{suffix}(source: {source_type}, inputs: &[{input_dto}<'_>]{context_arg}, loader: &dyn DynamicXmlSourceLoader) -> Result<{result}, codegen_runtime::XmlDynamicInputDocumentOutputsExecutionError> {{\n    let _count = codegen_runtime::XmlInputSetBudget::new(inputs.len().saturating_add(1)).map_err(xml_dynamic_document_outputs_input_error)?;\n    let supplied_names: Vec<&str> = inputs.iter().map(|input| input.name).collect();\n    let indices = codegen_runtime::xml_input_indices(EXTRA_SOURCE_NAMES, &supplied_names).map_err(xml_dynamic_document_outputs_input_error)?;\n    let mut sizes = Vec::with_capacity(EXTRA_SOURCE_NAMES.len() + 1);\n    sizes.push((codegen_runtime::XmlInputSource::Primary, source.len()));\n"));
            for (static_index, (index, input)) in static_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    sizes.push((codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}, inputs[indices[{static_index}]].document.len()));\n"));
            }
            output.push_str(&format!("    let _ = &indices;\n    let budget = codegen_runtime::preflight_xml_input_sizes_with_budget(&sizes).map_err(xml_dynamic_document_outputs_input_error)?;\n    let parsed = codegen_runtime::{parser}(SOURCE_XML_SCHEMA, source).map_err(|error| codegen_runtime::XmlDynamicInputDocumentOutputsExecutionError::from_input_boundary(Some(codegen_runtime::XmlInputSource::Primary), Box::new(error)))?;\n"));
            for (static_index, (index, input)) in static_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    let parsed_input_{index} = codegen_runtime::{parser}(EXTRA_XML_INPUT_SCHEMA_{index}, inputs[indices[{static_index}]].document).map_err(|error| codegen_runtime::XmlDynamicInputDocumentOutputsExecutionError::from_input_boundary(Some(codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}), Box::new(error)))?;\n"));
            }
            output.push_str("    let parsed_inputs: Vec<NamedInput<'_>> = vec![\n");
            for (index, input) in &static_sources {
                output.push_str(&format!(
                    "        NamedInput {{ name: {}, instance: &parsed_input_{index} }},\n",
                    rust_string(&input.name)
                ));
            }
            let dynamic_name = rust_string(&dynamic_source.name);
            output.push_str(&format!("    ];\n    let adapter = codegen_runtime::XmlDynamicSourceAdapter::new(loader, codegen_runtime::XmlDynamicSourcePolicy {{ declaration_index: {dynamic_index}, source: {dynamic_name}, schema: EXTRA_XML_INPUT_SCHEMA_{dynamic_index} }}, budget);\n    let mapped = {execute}(&parsed, &parsed_inputs{context_call}, &adapter).map_err(|error| xml_dynamic_document_outputs_recovery_error(adapter.recover(error)))?;\n    {helper}(mapped).map_err(codegen_runtime::XmlDynamicInputDocumentOutputsExecutionError::from)\n}}\n\n"));
        }
        let target_count = program.extra_targets.len();
        output.push_str(&format!(r#"fn {helper}(mapped: ExecutionOutputs) -> Result<{result}, codegen_runtime::XmlDocumentOutputsExecutionError> {{
    if !matches!(&mapped.primary, Instance::Group(_)) {{
        return Err(codegen_runtime::XmlDocumentOutputsExecutionError::alignment("XML document outputs require an ordinary primary group"));
    }}
    if mapped.extras.len() != {target_count} {{
        return Err(codegen_runtime::XmlDocumentOutputsExecutionError::alignment("XML document outputs do not match the declared named targets"));
    }}
    let mut named_targets = mapped.extras.into_iter();
"#));
        // Move every aligned document set before count arithmetic or serialization.
        for declaration_index in 0..target_count {
            output.push_str(&format!(r#"    let named_{declaration_index} = named_targets.next().ok_or_else(|| codegen_runtime::XmlDocumentOutputsExecutionError::alignment("XML document outputs require every declared named target"))?;
    if named_{declaration_index}.name != NAMED_XML_DOCUMENT_OUTPUT_NAME_{declaration_index} {{
        return Err(codegen_runtime::XmlDocumentOutputsExecutionError::alignment("XML document outputs do not match exact named declaration order"));
    }}
    let Instance::DocumentSet(members_{declaration_index}) = named_{declaration_index}.instance else {{
        return Err(codegen_runtime::XmlDocumentOutputsExecutionError::alignment("XML named document outputs require document sets"));
    }};
"#));
        }
        output.push_str("    let mut artifact_count = 1usize;\n");
        for declaration_index in 0..target_count {
            output.push_str(&format!(
                "    artifact_count = artifact_count.checked_add(members_{declaration_index}.len()).ok_or_else(|| codegen_runtime::XmlDocumentOutputsExecutionError::alignment(\"XML document output artifact count exceeds the host index range\"))?;\n",
            ));
        }
        output.push_str(&format!(r#"    let mut budget = codegen_runtime::XmlDocumentOutputsBudget::new(artifact_count)?;
    let primary_xml = codegen_runtime::serialize_xml_document(TARGET_XML_SCHEMA, &mapped.primary, {primary_arguments}).map_err(codegen_runtime::XmlDocumentOutputsExecutionError::primary_serialization)?;
    budget.charge_primary(primary_xml.len())?;
    let primary = {primary_conversion};
    let mut extras = Vec::with_capacity({target_count});
"#));
        for (declaration_index, named_policy) in policy.extra_outputs.iter().enumerate() {
            let named_arguments = output_arguments(&named_policy.output)?;
            output.push_str(&format!(r#"    let mut documents_{declaration_index} = Vec::with_capacity(members_{declaration_index}.len());
    for (index, member) in members_{declaration_index}.into_iter().enumerate() {{
        let xml = codegen_runtime::serialize_xml_document(NAMED_XML_DOCUMENT_SCHEMA_{declaration_index}, member.value(), {named_arguments}).map_err(|error| codegen_runtime::XmlDocumentOutputsExecutionError::named_serialization({declaration_index}, NAMED_XML_DOCUMENT_OUTPUT_NAME_{declaration_index}, index, member.path(), error))?;
        budget.charge_named({declaration_index}, NAMED_XML_DOCUMENT_OUTPUT_NAME_{declaration_index}, index, member.path(), xml.len())?;
        documents_{declaration_index}.push({dto} {{ path: member.path().to_owned(), document: {conversion} }});
    }}
    extras.push({named_dto} {{ name: NAMED_XML_DOCUMENT_OUTPUT_NAME_{declaration_index}, documents: documents_{declaration_index} }});
"#));
        }
        output.push_str(&format!("    Ok({result} {{ primary, extras }})\n}}\n\n"));
    }
    Ok(output)
}
