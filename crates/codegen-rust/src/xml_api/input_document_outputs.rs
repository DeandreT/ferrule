//! Closed static input admission followed by complete existing mixed XML serialization.
use super::output_arguments;
use crate::{EmitError, rust_string};
use codegen::{Program, ProgramValidationError};

pub(super) fn render(program: &Program) -> Result<String, EmitError> {
    if program.xml_output_mode()?
        != Some(codegen::XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments)
    {
        return Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "static input mixed XML outputs require their checked adapter mode".into(),
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
    output.push_str("\n#[derive(Debug, Clone, Copy)]\npub struct NamedXmlInput<'a> { pub name: &'a str, pub document: &'a str }\n#[derive(Debug, Clone, Copy)]\npub struct NamedXmlBytesInput<'a> { pub name: &'a str, pub document: &'a [u8] }\n\n");
    output.push_str("\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlDocumentOutput { pub path: String, pub document: String }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlBytesDocumentOutput { pub path: String, pub document: Vec<u8> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct NamedXmlDocumentOutputs { pub name: &'static str, pub documents: Vec<XmlDocumentOutput> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct NamedXmlBytesDocumentOutputs { pub name: &'static str, pub documents: Vec<XmlBytesDocumentOutput> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlDocumentExecutionOutputs { pub primary: String, pub extras: Vec<NamedXmlDocumentOutputs> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlBytesDocumentExecutionOutputs { pub primary: Vec<u8>, pub extras: Vec<NamedXmlBytesDocumentOutputs> }\n\n");
    output.push_str("// Only the static admission helpers below reach this private conversion.\nfn xml_document_outputs_input_error(error: codegen_runtime::XmlExecutionError) -> codegen_runtime::XmlInputDocumentOutputsExecutionError {\n    debug_assert!(error.output.is_none() && error.request.is_none());\n    codegen_runtime::XmlInputDocumentOutputsExecutionError::from_input_boundary(error.input, error.boundary)\n}\n\n");
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
            output.push_str(&format!("pub fn {stem}{suffix}(source: {source_type}, inputs: &[{input_dto}<'_>]{context_arg}) -> Result<{result}, codegen_runtime::XmlInputDocumentOutputsExecutionError> {{\n    let _count = codegen_runtime::XmlInputSetBudget::new(inputs.len().saturating_add(1)).map_err(xml_document_outputs_input_error)?;\n    let supplied_names: Vec<&str> = inputs.iter().map(|input| input.name).collect();\n    let indices = codegen_runtime::xml_input_indices(EXTRA_SOURCE_NAMES, &supplied_names).map_err(xml_document_outputs_input_error)?;\n    let mut sizes = Vec::with_capacity(EXTRA_SOURCE_NAMES.len() + 1);\n    sizes.push((codegen_runtime::XmlInputSource::Primary, source.len()));\n"));
            for (index, input) in program.extra_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    sizes.push((codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}, inputs[indices[{index}]].document.len()));\n"));
            }
            output.push_str(&format!("    codegen_runtime::preflight_xml_input_sizes(&sizes).map_err(xml_document_outputs_input_error)?;\n    let parsed = codegen_runtime::{parser}(SOURCE_XML_SCHEMA, source).map_err(|error| codegen_runtime::XmlInputDocumentOutputsExecutionError::from_input_boundary(Some(codegen_runtime::XmlInputSource::Primary), Box::new(error)))?;\n"));
            for (index, input) in program.extra_sources.iter().enumerate() {
                let name = rust_string(&input.name);
                output.push_str(&format!("    let parsed_input_{index} = codegen_runtime::{parser}(EXTRA_XML_INPUT_SCHEMA_{index}, inputs[indices[{index}]].document).map_err(|error| codegen_runtime::XmlInputDocumentOutputsExecutionError::from_input_boundary(Some(codegen_runtime::XmlInputSource::Named {{ index: {index}, name: {name} }}), Box::new(error)))?;\n"));
            }
            output.push_str("    let parsed_inputs: Vec<NamedInput<'_>> = vec![\n");
            for (index, input) in program.extra_sources.iter().enumerate() {
                output.push_str(&format!(
                    "        NamedInput {{ name: {}, instance: &parsed_input_{index} }},\n",
                    rust_string(&input.name)
                ));
            }
            output.push_str(&format!("    ];\n    let mapped = {execute}(&parsed, &parsed_inputs{context_call}).map_err(codegen_runtime::XmlBoundaryError::from).map_err(codegen_runtime::XmlInputDocumentOutputsExecutionError::from)?;\n    {helper}(mapped).map_err(codegen_runtime::XmlInputDocumentOutputsExecutionError::from)\n}}\n\n"));
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

#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> ::mapping::Project {
        serde_json::from_str(include_str!("../../../codegen/src/tests/fixtures/static_named_inputs_static_primary_dynamic_named_xml_documents.json")).unwrap()
    }
    fn program() -> Program {
        codegen::lower(&project()).unwrap()
    }

    #[test]
    fn exports_exactly_four_dedicated_sources_apis_with_existing_mixed_dtos() {
        let output = super::super::render(&program()).unwrap();
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
                "execute_xml_document_outputs_with_sources",
                "execute_xml_document_outputs_with_sources_and_context",
                "execute_xml_bytes_document_outputs_with_sources",
                "execute_xml_bytes_document_outputs_with_sources_and_context",
            ]
        );
        assert!(output.contains("pub struct NamedXmlInput<'a>"));
        assert!(output.contains("pub struct NamedXmlBytesInput<'a>"));
        assert!(output.contains("pub struct XmlDocumentExecutionOutputs { pub primary: String, pub extras: Vec<NamedXmlDocumentOutputs> }"));
        assert!(output.contains("pub struct XmlBytesDocumentExecutionOutputs { pub primary: Vec<u8>, pub extras: Vec<NamedXmlBytesDocumentOutputs> }"));
        assert!(output.contains("XmlInputDocumentOutputsExecutionError::from_input_boundary(error.input, error.boundary)"));
        assert!(!output.contains("pub fn execute_xml("));
        assert!(!output.contains("pub fn execute_xml_documents_with_sources("));
        assert!(!output.contains("pub fn execute_xml_document_outputs("));
        assert!(!output.contains("XmlDynamicSourceAdapter"));
    }

    #[test]
    fn every_original_input_and_target_keeps_its_own_schema_policy_and_index() {
        let mut project = project();
        for reversed in [false, true] {
            if reversed {
                project.extra_targets.swap(0, 1);
            }
            let program = codegen::lower(&project).unwrap();
            let output = render(&program).unwrap();
            for (index, input) in program.extra_sources.iter().enumerate() {
                let descriptor = codegen::serialize_embedded_schema(
                    &input.source,
                    codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
                )
                .unwrap();
                assert!(output.contains(&format!(
                    "const EXTRA_XML_INPUT_SCHEMA_{index}: &str = {};",
                    rust_string(&descriptor)
                )));
                assert!(output.contains(&format!(
                    "XmlInputSource::Named {{ index: {index}, name: {} }}",
                    rust_string(&input.name)
                )));
                assert!(output.contains(&format!("parse_structured_xml(EXTRA_XML_INPUT_SCHEMA_{index}, inputs[indices[{index}]].document)")));
                assert!(output.contains(&format!("parse_structured_xml_bytes(EXTRA_XML_INPUT_SCHEMA_{index}, inputs[indices[{index}]].document)")));
            }
            assert_eq!(program.extra_sources.len(), 3); // Includes unused-padding.
            for (index, target) in program.extra_targets.iter().enumerate() {
                let descriptor = codegen::serialize_embedded_schema(
                    &target.target,
                    codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
                )
                .unwrap();
                assert!(output.contains(&format!(
                    "const NAMED_XML_DOCUMENT_SCHEMA_{index}: &str = {};",
                    rust_string(&descriptor)
                )));
                assert!(output.contains(&format!(
                    "const NAMED_XML_DOCUMENT_OUTPUT_NAME_{index}: &str = {};",
                    rust_string(&target.name)
                )));
                let policy = &program.xml_boundary.as_ref().unwrap().extra_outputs[index].output;
                assert!(output.contains(&format!(
                    "serialize_xml_document(NAMED_XML_DOCUMENT_SCHEMA_{index}, member.value(), {})",
                    output_arguments(policy).unwrap()
                )));
                assert!(output.contains(&format!("named_serialization({index}, NAMED_XML_DOCUMENT_OUTPUT_NAME_{index}, index, member.path(), error)")));
            }
        }
    }

    #[test]
    fn complete_inputs_precede_mapping_and_alignment_count_precede_serialization() {
        let output = render(&program()).unwrap();
        let count = output
            .find("XmlInputSetBudget::new(inputs.len().saturating_add(1))")
            .unwrap();
        let names = output.find("xml_input_indices(EXTRA_SOURCE_NAMES").unwrap();
        let sizes = output.find("preflight_xml_input_sizes(&sizes)").unwrap();
        let primary = output
            .find("parse_structured_xml(SOURCE_XML_SCHEMA, source)")
            .unwrap();
        let last_input = output
            .find("parse_structured_xml(EXTRA_XML_INPUT_SCHEMA_2")
            .unwrap();
        let mapping = output
            .find("let mapped = execute_outputs_with_sources(&parsed")
            .unwrap();
        assert!(
            count < names
                && names < sizes
                && sizes < primary
                && primary < last_input
                && last_input < mapping
        );
        let last_alignment = output.find("Instance::DocumentSet(members_1)").unwrap();
        let first_count = output
            .find("artifact_count.checked_add(members_0.len())")
            .unwrap();
        let last_count = output
            .find("artifact_count.checked_add(members_1.len())")
            .unwrap();
        let budget = output
            .find("XmlDocumentOutputsBudget::new(artifact_count)")
            .unwrap();
        let serialize = output
            .find("serialize_xml_document(TARGET_XML_SCHEMA")
            .unwrap();
        let charge = output
            .find("budget.charge_primary(primary_xml.len())")
            .unwrap();
        let named = output
            .find("serialize_xml_document(NAMED_XML_DOCUMENT_SCHEMA_0")
            .unwrap();
        assert!(
            last_alignment < first_count
                && first_count < last_count
                && last_count < budget
                && budget < serialize
                && serialize < charge
                && charge < named
        );
        let named_charge = output.find("budget.charge_named(1,").unwrap();
        let retain = output.find("documents_1.push(").unwrap();
        assert!(named_charge < retain);
        assert!(output.contains("let primary = primary_xml.into_bytes();"));
        assert!(output.contains("document: xml.into_bytes()"));
    }

    #[test]
    fn malformed_later_policies_refuse_before_rendering_and_old_modes_stay_separate() {
        for input in [true, false] {
            let mut invalid = program();
            if input {
                invalid.xml_boundary.as_mut().unwrap().extra_inputs[2].name = "wrong".into();
            } else {
                invalid.xml_boundary.as_mut().unwrap().extra_outputs[1].name = "wrong".into();
            }
            assert!(matches!(
                super::super::render(&invalid),
                Err(EmitError::InvalidProgram(_))
            ));
        }
        for (fixture, expected) in [
            (
                include_str!(
                    "../../../codegen/src/tests/fixtures/static_named_inputs_dynamic_primary_xml_documents.json"
                ),
                "pub fn execute_xml_documents_with_sources(",
            ),
            (
                include_str!(
                    "../../../codegen/src/tests/fixtures/static_primary_multiple_dynamic_named_xml_documents.json"
                ),
                "pub fn execute_xml_document_outputs(",
            ),
        ] {
            let project: ::mapping::Project = serde_json::from_str(fixture).unwrap();
            let output = super::super::render(&codegen::lower(&project).unwrap()).unwrap();
            assert!(output.contains(expected));
            assert!(!output.contains("execute_xml_document_outputs_with_sources"));
            assert!(!output.contains("XmlInputDocumentOutputsExecutionError"));
        }
    }
}
