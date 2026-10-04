mod named_inputs;

use crate::{EmitError, rust_string};
use codegen::{Program, XmlOutputPolicy};

fn output_arguments(policy: &XmlOutputPolicy) -> Result<String, EmitError> {
    let namespace = policy
        .default_namespace
        .as_ref()
        .map(|uri| format!("Some({})", rust_string(uri)))
        .unwrap_or_else(|| "None".into());
    let hints = policy
        .schema_hints
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| EmitError::SchemaSerialization(error.to_string()))?
        .map(|json| format!("Some({})", rust_string(&json)))
        .unwrap_or_else(|| "None".into());
    Ok(format!(
        "{}, {}, {namespace}, {hints}",
        policy.declaration, policy.indent
    ))
}

pub(crate) fn render(program: &Program) -> Result<String, EmitError> {
    let Some(policy) = &program.xml_boundary else {
        return Ok(String::new());
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
    for (index, target) in program.extra_targets.iter().enumerate() {
        let descriptor = codegen::serialize_embedded_schema(
            &target.target,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?;
        output.push_str(&format!(
            "const EXTRA_XML_SCHEMA_{index}: &str = {};\n",
            rust_string(&descriptor)
        ));
    }
    let names = program
        .extra_targets
        .iter()
        .map(|target| rust_string(&target.name))
        .collect::<Vec<_>>()
        .join(", ");
    output.push_str(&format!(
        "const EXTRA_XML_OUTPUT_NAMES: &[&str] = &[{names}];\n"
    ));
    output.push_str("\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct NamedXmlOutput { pub name: &'static str, pub document: String }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlExecutionOutputs { pub primary: String, pub extras: Vec<NamedXmlOutput> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct NamedXmlBytesOutput { pub name: &'static str, pub document: Vec<u8> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlBytesExecutionOutputs { pub primary: Vec<u8>, pub extras: Vec<NamedXmlBytesOutput> }\n\n");
    if policy.input.profile() == Some(codegen::XmlInputProfile::Structured) {
        output.push_str(&named_inputs::render(program)?);
    }
    for bytes in [false, true] {
        let stem = if bytes {
            "execute_xml_bytes"
        } else {
            "execute_xml"
        };
        let result_type = if bytes {
            "XmlBytesExecutionOutputs"
        } else {
            "XmlExecutionOutputs"
        };
        let scalar_type = if bytes { "Vec<u8>" } else { "String" };
        let source_type = if bytes { "&[u8]" } else { "&str" };
        let helper = if bytes {
            "serialize_xml_bytes_outputs"
        } else {
            "serialize_xml_outputs"
        };
        for context in [false, true] {
            if policy.input.profile() == Some(codegen::XmlInputProfile::Structured) {
                continue;
            }
            let suffix = if context { "_with_context" } else { "" };
            let name = format!("{stem}_outputs{suffix}");
            let singular = format!("{stem}{suffix}");
            let context_arg = if context {
                ", execution: &ExecutionContext<'_>"
            } else {
                ""
            };
            let context_call = if context { ", execution" } else { "" };
            let execute = if context {
                "execute_outputs_with_context"
            } else {
                "execute_outputs"
            };
            let (parser, input_args) = match policy.input.profile() {
                Some(codegen::XmlInputProfile::RootView) => (
                    if bytes {
                        "parse_xml_bytes"
                    } else {
                        "parse_xml"
                    },
                    format!(
                        ", {}, {}",
                        policy.input.allow_inactive_root_type_members,
                        policy.input.root_view_policy
                    ),
                ),
                Some(codegen::XmlInputProfile::Structured) => (
                    if bytes {
                        "parse_structured_xml_bytes"
                    } else {
                        "parse_structured_xml"
                    },
                    String::new(),
                ),
                None => {
                    return Err(EmitError::InvalidProgram(
                        codegen::ProgramValidationError::InvalidXmlBoundary {
                            reason: "mixed XML input profile flags".into(),
                        },
                    ));
                }
            };
            output.push_str(&format!("pub fn {name}(source: {source_type}{context_arg}) -> Result<{result_type}, codegen_runtime::XmlOutputSetError> {{\n    let parsed = codegen_runtime::{parser}(SOURCE_XML_SCHEMA, source{input_args}).map_err(codegen_runtime::XmlOutputSetError::from)?;\n    let mapped = {execute}(&parsed{context_call}).map_err(codegen_runtime::XmlBoundaryError::from).map_err(codegen_runtime::XmlOutputSetError::from)?;\n    {helper}(mapped)\n}}\n\npub fn {singular}(source: {source_type}{context_arg}) -> Result<{scalar_type}, codegen_runtime::XmlBoundaryError> {{\n    {name}(source{context_call}).map(|outputs| outputs.primary).map_err(codegen_runtime::XmlOutputSetError::into_boundary)\n}}\n\n"));
        }
        output.push_str(&format!("fn {helper}(mapped: ExecutionOutputs) -> Result<{result_type}, codegen_runtime::XmlOutputSetError> {{\n    if mapped.extras.len() != EXTRA_XML_OUTPUT_NAMES.len() || mapped.extras.iter().zip(EXTRA_XML_OUTPUT_NAMES).any(|(target, name)| target.name != *name) {{\n        return Err(codegen_runtime::XmlOutputSetError::alignment(\"XML mapped outputs do not match declared target names/order\"));\n    }}\n    let mut budget = codegen_runtime::XmlOutputSetBudget::new(mapped.extras.len() + 1)?;\n"));
        let args = output_arguments(&policy.output)?;
        output.push_str(&format!("    let primary_xml = codegen_runtime::serialize_xml_document(TARGET_XML_SCHEMA, &mapped.primary, {args}).map_err(|error| codegen_runtime::XmlOutputSetError::new(Some(codegen_runtime::XmlOutputTarget::Primary), error))?;\n    budget.charge(codegen_runtime::XmlOutputTarget::Primary, primary_xml.len())?;\n    let primary = {};\n", if bytes { "primary_xml.into_bytes()" } else { "primary_xml" }));
        if program.extra_targets.is_empty() {
            output.push_str("    let extras = Vec::new();\n");
        } else {
            output.push_str(&format!(
                "    let mut extras = Vec::with_capacity({});\n",
                program.extra_targets.len()
            ));
        }
        for (index, (target, policy)) in program
            .extra_targets
            .iter()
            .zip(&policy.extra_outputs)
            .enumerate()
        {
            let name = rust_string(&target.name);
            let args = output_arguments(&policy.output)?;
            let dto = if bytes {
                "NamedXmlBytesOutput"
            } else {
                "NamedXmlOutput"
            };
            output.push_str(&format!("    let target = codegen_runtime::XmlOutputTarget::Named {{ index: {index}, name: {name} }};\n    let xml = codegen_runtime::serialize_xml_document(EXTRA_XML_SCHEMA_{index}, &mapped.extras[{index}].instance, {args}).map_err(|error| codegen_runtime::XmlOutputSetError::new(Some(target), error))?;\n    budget.charge(target, xml.len())?;\n    extras.push({dto} {{ name: {name}, document: {} }});\n", if bytes { "xml.into_bytes()" } else { "xml" }));
        }
        output.push_str(&format!(
            "    Ok({result_type} {{ primary, extras }})\n}}\n\n"
        ));
    }
    Ok(output)
}
