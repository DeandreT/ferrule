//! Separate static-input admission and dynamic-primary document-list renderer.
use super::output_arguments;
use crate::{EmitError, rust_string};
use codegen::{Program, ProgramValidationError, XmlOutputMode};

pub(super) fn render(program: &Program) -> Result<String, EmitError> {
    if program.xml_output_mode()? != Some(XmlOutputMode::StaticNamedInputsDynamicPrimaryDocuments) {
        return Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "static XML input document lists require their checked adapter mode".into(),
        }
        .into());
    }
    let policy = program.xml_boundary.as_ref().ok_or_else(|| {
        ProgramValidationError::InvalidXmlBoundary {
            reason: "missing static-input document-list XML boundary".into(),
        }
    })?;
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
    output.push_str("\n#[derive(Debug, Clone, Copy)]\npub struct NamedXmlInput<'a> { pub name: &'a str, pub document: &'a str }\n#[derive(Debug, Clone, Copy)]\npub struct NamedXmlBytesInput<'a> { pub name: &'a str, pub document: &'a [u8] }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlDocumentOutput { pub path: String, pub document: String }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlBytesDocumentOutput { pub path: String, pub document: Vec<u8> }\n\n");
    output.push_str("// Only the static admission helpers below reach this private conversion.\nfn xml_document_input_error(error: codegen_runtime::XmlExecutionError) -> codegen_runtime::XmlInputDocumentExecutionError {\n    debug_assert!(error.output.is_none() && error.request.is_none());\n    codegen_runtime::XmlInputDocumentExecutionError::from_input_boundary(error.input, error.boundary)\n}\n\n");
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
            "serialize_xml_bytes_document_members_with_inputs"
        } else {
            "serialize_xml_document_members_with_inputs"
        };
        for context in [false, true] {
            let suffix = if context { "_and_context" } else { "" };
            let context_arg = if context {
                ", execution: &codegen_runtime::ExecutionContext<'_>"
            } else {
                ""
            };
            let context_call = if context { ", execution" } else { "" };
            let execute = if context {
                "execute_outputs_with_sources_and_context"
            } else {
                "execute_outputs_with_sources"
            };
            output.push_str(&format!("pub fn {stem}{suffix}(source: {source_type}, inputs: &[{input_dto}<'_>]{context_arg}) -> Result<Vec<{dto}>, codegen_runtime::XmlInputDocumentExecutionError> {{\n    let _count = codegen_runtime::XmlInputSetBudget::new(inputs.len().saturating_add(1)).map_err(xml_document_input_error)?;\n    let supplied_names: Vec<&str> = inputs.iter().map(|input| input.name).collect();\n    let indices = codegen_runtime::xml_input_indices(EXTRA_SOURCE_NAMES, &supplied_names).map_err(xml_document_input_error)?;\n    let mut sizes = Vec::with_capacity(EXTRA_SOURCE_NAMES.len() + 1);\n    sizes.push((codegen_runtime::XmlInputSource::Primary, source.len()));\n"));
            for (index, input) in program.extra_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    sizes.push((codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}, inputs[indices[{index}]].document.len()));\n"));
            }
            output.push_str(&format!("    codegen_runtime::preflight_xml_input_sizes(&sizes).map_err(xml_document_input_error)?;\n    let parsed = codegen_runtime::{parser}(SOURCE_XML_SCHEMA, source).map_err(|error| codegen_runtime::XmlInputDocumentExecutionError::from_input_boundary(Some(codegen_runtime::XmlInputSource::Primary), Box::new(error)))?;\n"));
            for (index, input) in program.extra_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    let parsed_input_{index} = codegen_runtime::{parser}(EXTRA_XML_INPUT_SCHEMA_{index}, inputs[indices[{index}]].document).map_err(|error| codegen_runtime::XmlInputDocumentExecutionError::from_input_boundary(Some(codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}), Box::new(error)))?;\n"));
            }
            output.push_str("    let parsed_inputs: Vec<NamedInput<'_>> = vec![\n");
            for (index, input) in program.extra_sources.iter().enumerate() {
                output.push_str(&format!(
                    "        NamedInput {{ name: {}, instance: &parsed_input_{index} }},\n",
                    rust_string(&input.name)
                ));
            }
            output.push_str(&format!("    ];\n    let mapped = {execute}(&parsed, &parsed_inputs{context_call}).map_err(codegen_runtime::XmlBoundaryError::from).map_err(codegen_runtime::XmlInputDocumentExecutionError::from)?;\n    {helper}(mapped)\n}}\n\n"));
        }
        output.push_str(&format!("fn {helper}(mapped: ExecutionOutputs) -> Result<Vec<{dto}>, codegen_runtime::XmlInputDocumentExecutionError> {{\n    if !mapped.extras.is_empty() {{\n        return Err(codegen_runtime::XmlDocumentExecutionError::alignment(\"dynamic XML documents require no named mapped outputs\").into());\n    }}\n    let Instance::DocumentSet(members) = mapped.primary else {{\n        return Err(codegen_runtime::XmlDocumentExecutionError::alignment(\"dynamic XML documents require a primary document set\").into());\n    }};\n    let mut budget = codegen_runtime::XmlDocumentSetBudget::new(members.len()).map_err(codegen_runtime::XmlInputDocumentExecutionError::from)?;\n    let mut outputs = Vec::with_capacity(members.len());\n    for (index, member) in members.into_iter().enumerate() {{\n        let xml = codegen_runtime::serialize_xml_document(TARGET_XML_SCHEMA, member.value(), {arguments}).map_err(|error| codegen_runtime::XmlInputDocumentExecutionError::from(codegen_runtime::XmlDocumentExecutionError::serialization(index, member.path(), error)))?;\n        budget.charge(index, member.path(), xml.len()).map_err(codegen_runtime::XmlInputDocumentExecutionError::from)?;\n        outputs.push({dto} {{ path: member.path().to_owned(), document: {} }});\n    }}\n    Ok(outputs)\n}}\n\n", if bytes { "xml.into_bytes()" } else { "xml" }));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn program() -> Program {
        let project: ::mapping::Project = serde_json::from_str(include_str!("../../../codegen/src/tests/fixtures/static_named_inputs_dynamic_primary_xml_documents.json")).unwrap();
        codegen::lower(&project).unwrap()
    }
    #[test]
    fn emits_only_four_with_sources_list_apis_and_original_input_owners() {
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
                "execute_xml_documents_with_sources",
                "execute_xml_documents_with_sources_and_context",
                "execute_xml_bytes_documents_with_sources",
                "execute_xml_bytes_documents_with_sources_and_context"
            ]
        );
        assert!(output.contains("XmlInputSource::Named { index: 1, name: \"beta\" }"));
        assert!(output.contains("EXTRA_XML_INPUT_SCHEMA_0"));
        assert!(output.contains("EXTRA_XML_INPUT_SCHEMA_1"));
        assert!(output.contains("let Instance::DocumentSet(members)"));
        assert!(!output.contains("pub fn execute_xml("));
        assert!(!output.contains("pub fn execute_xml_documents("));
        assert!(!output.contains("XmlDynamicSourceAdapter"));
    }
    #[test]
    fn invalid_later_input_policy_refuses_and_old_modes_use_unchanged_dispatch() {
        let mut invalid = program();
        invalid.xml_boundary.as_mut().unwrap().extra_inputs[1].name = "wrong".into();
        assert!(render(&invalid).is_err());
        invalid.xml_boundary = None;
        assert!(super::super::render(&invalid).unwrap().is_empty());
        let project: ::mapping::Project = serde_json::from_str(include_str!(
            "../../../codegen/src/tests/fixtures/dynamic_primary_xml_documents.json"
        ))
        .unwrap();
        let old = codegen::lower(&project).unwrap();
        let source = super::super::render(&old).unwrap();
        assert!(source.contains("pub fn execute_xml_documents("));
        assert!(!source.contains("execute_xml_documents_with_sources"));
    }
}
