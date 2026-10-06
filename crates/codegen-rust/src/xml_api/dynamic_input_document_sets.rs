//! One-loader/static-input admission and dynamic-primary document-list renderer.
use super::output_arguments;
use crate::{EmitError, rust_string};
use codegen::{Program, ProgramValidationError, XmlOutputMode};

pub(super) fn render(program: &Program) -> Result<String, EmitError> {
    if program.xml_output_mode()? != Some(XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments) {
        return Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "dynamic-input XML document lists require their checked adapter mode".into(),
        }
        .into());
    }
    let policy = program.xml_boundary.as_ref().ok_or_else(|| {
        ProgramValidationError::InvalidXmlBoundary {
            reason: "missing dynamic-input document-list XML boundary".into(),
        }
    })?;
    let static_sources = program
        .extra_sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.dynamic.is_none())
        .collect::<Vec<_>>();
    let dynamic_sources = program
        .extra_sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.dynamic.is_some())
        .collect::<Vec<_>>();
    let (dynamic_index, dynamic_source) = dynamic_sources.first().copied().ok_or_else(|| {
        ProgramValidationError::InvalidXmlBoundary {
            reason: "dynamic-input document lists require their dynamic declaration".into(),
        }
    })?;
    let adapter = if dynamic_sources.len() == 1 {
        format!(
            "    let adapter = codegen_runtime::XmlDynamicSourceAdapter::new(loader, codegen_runtime::XmlDynamicSourcePolicy {{ declaration_index: {dynamic_index}, source: {}, schema: EXTRA_XML_INPUT_SCHEMA_{dynamic_index} }}, budget);\n",
            rust_string(&dynamic_source.name)
        )
    } else {
        let mut adapter = String::from(
            "    let adapter = codegen_runtime::XmlDynamicSourceAdapter::for_sources(loader, vec![\n",
        );
        for (index, source) in dynamic_sources {
            adapter.push_str(&format!("        codegen_runtime::XmlDynamicSourcePolicy {{ declaration_index: {index}, source: {}, schema: EXTRA_XML_INPUT_SCHEMA_{index} }},\n", rust_string(&source.name)));
        }
        adapter.push_str("    ], budget);\n");
        adapter
    };
    let source = codegen::serialize_embedded_schema(
        &program.source,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let target = codegen::serialize_embedded_schema(
        &program.target,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let mut output = format!(
        "\nconst SOURCE_XML_SCHEMA: &str = {};\nconst TARGET_XML_SCHEMA: &str = {};\n",
        rust_string(&source),
        rust_string(&target)
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
    output.push_str("pub use codegen_runtime::DynamicXmlSourceLoader;\n\n");
    output.push_str("\n#[derive(Debug, Clone, Copy)]\npub struct NamedXmlInput<'a> { pub name: &'a str, pub document: &'a str }\n#[derive(Debug, Clone, Copy)]\npub struct NamedXmlBytesInput<'a> { pub name: &'a str, pub document: &'a [u8] }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlDocumentOutput { pub path: String, pub document: String }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlBytesDocumentOutput { pub path: String, pub document: Vec<u8> }\n\n");
    output.push_str(r#"// Only static admission and synchronous adapter recovery reach this conversion.
fn xml_dynamic_document_input_error(error: codegen_runtime::XmlExecutionError) -> codegen_runtime::XmlDynamicInputDocumentExecutionError {
    debug_assert!(error.output.is_none());
    match error.request {
        Some(request) => {
            debug_assert_eq!(error.input, Some(codegen_runtime::XmlInputSource::Named {
                index: request.declaration_index, name: request.source,
            }));
            codegen_runtime::XmlDynamicInputDocumentExecutionError::from_dynamic_input_boundary(request, error.boundary)
        }
        None => codegen_runtime::XmlDynamicInputDocumentExecutionError::from_input_boundary(error.input, error.boundary),
    }
}

"#);
    let arguments = output_arguments(&policy.output)?;
    for bytes in [false, true] {
        let stem = if bytes {
            "execute_xml_bytes_documents_with_sources"
        } else {
            "execute_xml_documents_with_sources"
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
        let helper = if bytes {
            "serialize_xml_bytes_document_members_with_dynamic_input"
        } else {
            "serialize_xml_document_members_with_dynamic_input"
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
            output.push_str(&format!("pub fn {stem}{suffix}(source: {source_type}, inputs: &[{input_dto}<'_>]{context_arg}, loader: &dyn DynamicXmlSourceLoader) -> Result<Vec<{dto}>, codegen_runtime::XmlDynamicInputDocumentExecutionError> {{\n    let _count = codegen_runtime::XmlInputSetBudget::new(inputs.len().saturating_add(1)).map_err(xml_dynamic_document_input_error)?;\n    let supplied_names: Vec<&str> = inputs.iter().map(|input| input.name).collect();\n    let indices = codegen_runtime::xml_input_indices(EXTRA_SOURCE_NAMES, &supplied_names).map_err(xml_dynamic_document_input_error)?;\n    let mut sizes = Vec::with_capacity(EXTRA_SOURCE_NAMES.len() + 1);\n    sizes.push((codegen_runtime::XmlInputSource::Primary, source.len()));\n"));
            for (static_index, (index, input)) in static_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    sizes.push((codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}, inputs[indices[{static_index}]].document.len()));\n"));
            }
            output.push_str(&format!("    let _ = &indices;\n    let budget = codegen_runtime::preflight_xml_input_sizes_with_budget(&sizes).map_err(xml_dynamic_document_input_error)?;\n    let parsed = codegen_runtime::{parser}(SOURCE_XML_SCHEMA, source).map_err(|error| codegen_runtime::XmlDynamicInputDocumentExecutionError::from_input_boundary(Some(codegen_runtime::XmlInputSource::Primary), Box::new(error)))?;\n"));
            for (static_index, (index, input)) in static_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    let parsed_input_{index} = codegen_runtime::{parser}(EXTRA_XML_INPUT_SCHEMA_{index}, inputs[indices[{static_index}]].document).map_err(|error| codegen_runtime::XmlDynamicInputDocumentExecutionError::from_input_boundary(Some(codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}), Box::new(error)))?;\n"));
            }
            output.push_str("    let parsed_inputs: Vec<NamedInput<'_>> = vec![\n");
            for (index, input) in &static_sources {
                output.push_str(&format!(
                    "        NamedInput {{ name: {}, instance: &parsed_input_{index} }},\n",
                    rust_string(&input.name)
                ));
            }
            output.push_str(&format!("    ];\n{adapter}    let mapped = {execute}(&parsed, &parsed_inputs{context_call}, &adapter).map_err(|error| xml_dynamic_document_input_error(adapter.recover(error)))?;\n    {helper}(mapped)\n}}\n\n"));
        }
        output.push_str(&format!("fn {helper}(mapped: ExecutionOutputs) -> Result<Vec<{dto}>, codegen_runtime::XmlDynamicInputDocumentExecutionError> {{\n    if !mapped.extras.is_empty() {{\n        return Err(codegen_runtime::XmlDocumentExecutionError::alignment(\"dynamic XML documents require no named mapped outputs\").into());\n    }}\n    let Instance::DocumentSet(members) = mapped.primary else {{\n        return Err(codegen_runtime::XmlDocumentExecutionError::alignment(\"dynamic XML documents require a primary document set\").into());\n    }};\n    let mut budget = codegen_runtime::XmlDocumentSetBudget::new(members.len()).map_err(codegen_runtime::XmlDynamicInputDocumentExecutionError::from)?;\n    let mut outputs = Vec::with_capacity(members.len());\n    for (index, member) in members.into_iter().enumerate() {{\n        let xml = codegen_runtime::serialize_xml_document(TARGET_XML_SCHEMA, member.value(), {arguments}).map_err(|error| codegen_runtime::XmlDynamicInputDocumentExecutionError::from(codegen_runtime::XmlDocumentExecutionError::serialization(index, member.path(), error)))?;\n        budget.charge(index, member.path(), xml.len()).map_err(codegen_runtime::XmlDynamicInputDocumentExecutionError::from)?;\n        outputs.push({dto} {{ path: member.path().to_owned(), document: {} }});\n    }}\n    Ok(outputs)\n}}\n\n", if bytes { "xml.into_bytes()" } else { "xml" }));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn program() -> Program {
        let project: ::mapping::Project = serde_json::from_str(include_str!("../../../codegen/src/tests/fixtures/one_dynamic_named_input_dynamic_primary_xml_documents.json")).unwrap();
        codegen::lower(&project).unwrap()
    }
    #[test]
    fn new_mode_emits_only_four_loader_lists_and_keeps_complete_declaration_indices() {
        let output = render(&program()).unwrap();
        let functions = output
            .lines()
            .filter_map(|line| {
                line.strip_prefix("pub fn ")
                    .map(|rest| rest.split('(').next().unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            functions,
            [
                "execute_xml_documents_with_sources_and_dynamic_source_loader",
                "execute_xml_documents_with_sources_context_and_dynamic_source_loader",
                "execute_xml_bytes_documents_with_sources_and_dynamic_source_loader",
                "execute_xml_bytes_documents_with_sources_context_and_dynamic_source_loader"
            ]
        );
        assert!(output.contains("pub use codegen_runtime::DynamicXmlSourceLoader;"));
        assert!(output.contains(
            "declaration_index: 1, source: \"catalog\", schema: EXTRA_XML_INPUT_SCHEMA_1"
        ));
        assert!(
            output.contains(
                "XmlInputSource::Named { index: 2, name: \"labels\" }, inputs[indices[1]]"
            )
        );
        assert!(!output.contains("pub fn execute_xml_documents("));
        assert!(!output.contains("pub fn execute_xml("));
        assert!(!output.contains("NamedInput { name: \"catalog\""));
        assert!(output.contains("from_dynamic_input_boundary(request, error.boundary)"));
        assert!(output.contains("adapter.recover(error)"));
    }
    #[test]
    fn complete_static_admission_and_mapping_precede_final_count_writer_charge_and_retention() {
        let output = render(&program()).unwrap();
        let positions = [
            "XmlInputSetBudget::new(inputs.len().saturating_add(1))",
            "xml_input_indices",
            "preflight_xml_input_sizes_with_budget",
            "let parsed =",
            "let parsed_input_2 =",
            "let adapter =",
            "let mapped =",
            "let Instance::DocumentSet(members)",
            "XmlDocumentSetBudget::new(members.len())",
            "Vec::with_capacity(members.len())",
            "serialize_xml_document(TARGET_XML_SCHEMA",
            "budget.charge(index, member.path()",
            "outputs.push",
        ]
        .map(|needle| output.find(needle).unwrap());
        assert!(positions.windows(2).all(|window| window[0] < window[1]));
        let bytes = output.rfind("budget.charge(index, member.path()").unwrap();
        assert!(bytes < output.rfind("xml.into_bytes()").unwrap());
        let mut invalid = program();
        invalid.xml_boundary.as_mut().unwrap().extra_inputs[2].name = "wrong".into();
        assert!(matches!(
            render(&invalid),
            Err(EmitError::InvalidProgram(_))
        ));
        invalid.xml_boundary = None;
        assert!(super::super::render(&invalid).unwrap().is_empty());
    }
    #[test]
    fn zero_static_subsequence_uses_no_dynamic_placeholder_instance() {
        let mut project: ::mapping::Project = serde_json::from_str(include_str!("../../../codegen/src/tests/fixtures/one_dynamic_named_input_dynamic_primary_xml_documents.json")).unwrap();
        project
            .extra_sources
            .retain(|source| source.dynamic_path.is_some());
        project.graph.nodes.insert(
            6,
            ::mapping::Node::Const {
                value: ir::Value::Float(2.0),
            },
        );
        project.graph.nodes.insert(
            10,
            ::mapping::Node::Const {
                value: ir::Value::String("literal".into()),
            },
        );
        let output = render(&codegen::lower(&project).unwrap()).unwrap();
        assert!(output.contains(
            "declaration_index: 0, source: \"catalog\", schema: EXTRA_XML_INPUT_SCHEMA_0"
        ));
        assert!(output.contains("let _ = &indices;"));
        assert!(!output.contains("let parsed_input_"));
        assert!(output.contains("let parsed_inputs: Vec<NamedInput<'_>> = vec![\n    ];"));
    }
    #[test]
    fn multiple_dynamic_policies_share_one_adapter_and_keep_complete_indices() {
        let project: ::mapping::Project = serde_json::from_str(include_str!("../../../codegen/src/tests/fixtures/multiple_dynamic_named_inputs_dynamic_primary_xml_documents.json")).unwrap();
        let mut program = codegen::lower(&project).unwrap();
        let output = render(&program).unwrap();
        assert_eq!(
            output
                .matches("XmlDynamicSourceAdapter::for_sources(loader, vec![")
                .count(),
            4
        );
        for policy in [
            "declaration_index: 1, source: \"catalog\", schema: EXTRA_XML_INPUT_SCHEMA_1",
            "declaration_index: 3, source: \"codes\", schema: EXTRA_XML_INPUT_SCHEMA_3",
        ] {
            assert_eq!(output.matches(policy).count(), 4);
        }
        assert!(
            output.contains(
                "XmlInputSource::Named { index: 2, name: \"labels\" }, inputs[indices[1]]"
            )
        );
        assert!(!output.contains("NamedInput { name: \"codes\""));
        assert_eq!(output.matches("adapter.recover(error)").count(), 4);
        let single = render(&self::program()).unwrap();
        assert_eq!(
            single
                .matches("XmlDynamicSourceAdapter::new(loader,")
                .count(),
            4
        );
        assert!(!single.contains("XmlDynamicSourceAdapter::for_sources("));
        program.xml_boundary.as_mut().unwrap().extra_inputs[3].name = "wrong".into();
        assert!(render(&program).is_err());
    }
}
