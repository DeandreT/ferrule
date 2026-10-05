use super::output_arguments;
use crate::{EmitError, rust_string};
use codegen::{Program, ProgramValidationError};

pub(super) fn render(program: &Program) -> Result<String, EmitError> {
    let policy = program.xml_boundary.as_ref().ok_or_else(|| {
        ProgramValidationError::InvalidXmlBoundary {
            reason: "XML document outputs require a boundary".into(),
        }
    })?;
    let named = program.extra_targets.first().ok_or_else(|| {
        ProgramValidationError::InvalidXmlBoundary {
            reason: "XML document outputs require one named target".into(),
        }
    })?;
    let named_policy =
        policy
            .extra_outputs
            .first()
            .ok_or_else(|| ProgramValidationError::InvalidXmlBoundary {
                reason: "XML document outputs require the named output policy".into(),
            })?;
    let source = codegen::serialize_embedded_schema(
        &program.source,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let primary = codegen::serialize_embedded_schema(
        &program.target,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let target =
        codegen::serialize_embedded_schema(&named.target, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?;
    let mut output = format!(
        "\nconst SOURCE_XML_SCHEMA: &str = {};\nconst TARGET_XML_SCHEMA: &str = {};\nconst NAMED_XML_DOCUMENT_SCHEMA: &str = {};\nconst NAMED_XML_DOCUMENT_OUTPUT_NAME: &str = {};\n",
        rust_string(&source),
        rust_string(&primary),
        rust_string(&target),
        rust_string(&named.name),
    );
    output.push_str("\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlDocumentOutput { pub path: String, pub document: String }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlBytesDocumentOutput { pub path: String, pub document: Vec<u8> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct NamedXmlDocumentOutputs { pub name: &'static str, pub documents: Vec<XmlDocumentOutput> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct NamedXmlBytesDocumentOutputs { pub name: &'static str, pub documents: Vec<XmlBytesDocumentOutput> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlDocumentExecutionOutputs { pub primary: String, pub extras: Vec<NamedXmlDocumentOutputs> }\n#[derive(Debug, Clone, PartialEq, Eq)]\npub struct XmlBytesDocumentExecutionOutputs { pub primary: Vec<u8>, pub extras: Vec<NamedXmlBytesDocumentOutputs> }\n\n");
    let primary_arguments = output_arguments(&policy.output)?;
    let named_arguments = output_arguments(&named_policy.output)?;
    for bytes in [false, true] {
        let stem = if bytes {
            "execute_xml_bytes_document_outputs"
        } else {
            "execute_xml_document_outputs"
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
            let suffix = if context { "_with_context" } else { "" };
            let context_argument = if context {
                ", execution: &codegen_runtime::ExecutionContext<'_>"
            } else {
                ""
            };
            let context_call = if context { ", execution" } else { "" };
            let execute = if context {
                "execute_outputs_with_context"
            } else {
                "execute_outputs"
            };
            output.push_str(&format!(r#"pub fn {stem}{suffix}(source: {source_type}{context_argument}) -> Result<{result}, codegen_runtime::XmlDocumentOutputsExecutionError> {{
    let parsed = codegen_runtime::{parser}(SOURCE_XML_SCHEMA, source).map_err(codegen_runtime::XmlDocumentOutputsExecutionError::from)?;
    let mapped = {execute}(&parsed{context_call}).map_err(codegen_runtime::XmlBoundaryError::from).map_err(codegen_runtime::XmlDocumentOutputsExecutionError::from)?;
    {helper}(mapped)
}}

"#));
        }
        output.push_str(&format!(r#"fn {helper}(mapped: ExecutionOutputs) -> Result<{result}, codegen_runtime::XmlDocumentOutputsExecutionError> {{
    if !matches!(&mapped.primary, Instance::Group(_)) {{
        return Err(codegen_runtime::XmlDocumentOutputsExecutionError::alignment("XML document outputs require an ordinary primary group"));
    }}
    if mapped.extras.len() != 1 || mapped.extras[0].name != NAMED_XML_DOCUMENT_OUTPUT_NAME {{
        return Err(codegen_runtime::XmlDocumentOutputsExecutionError::alignment("XML document outputs do not match the declared named target"));
    }}
    let named = mapped.extras.into_iter().next().ok_or_else(|| codegen_runtime::XmlDocumentOutputsExecutionError::alignment("XML document outputs require one named target"))?;
    let Instance::DocumentSet(members) = named.instance else {{
        return Err(codegen_runtime::XmlDocumentOutputsExecutionError::alignment("XML named document outputs require a document set"));
    }};
    let artifact_count = members.len().checked_add(1).ok_or_else(|| codegen_runtime::XmlDocumentOutputsExecutionError::alignment("XML document output artifact count exceeds the host index range"))?;
    let mut budget = codegen_runtime::XmlDocumentOutputsBudget::new(artifact_count)?;
    let primary_xml = codegen_runtime::serialize_xml_document(TARGET_XML_SCHEMA, &mapped.primary, {primary_arguments}).map_err(codegen_runtime::XmlDocumentOutputsExecutionError::primary_serialization)?;
    budget.charge_primary(primary_xml.len())?;
    let primary = {primary_conversion};
    let mut documents = Vec::with_capacity(members.len());
    for (index, member) in members.into_iter().enumerate() {{
        let xml = codegen_runtime::serialize_xml_document(NAMED_XML_DOCUMENT_SCHEMA, member.value(), {named_arguments}).map_err(|error| codegen_runtime::XmlDocumentOutputsExecutionError::named_serialization(0, NAMED_XML_DOCUMENT_OUTPUT_NAME, index, member.path(), error))?;
        budget.charge_named(0, NAMED_XML_DOCUMENT_OUTPUT_NAME, index, member.path(), xml.len())?;
        documents.push({dto} {{ path: member.path().to_owned(), document: {conversion} }});
    }}
    Ok({result} {{ primary, extras: vec![{named_dto} {{ name: NAMED_XML_DOCUMENT_OUTPUT_NAME, documents }}] }})
}}

"#));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixed_program() -> Program {
        let project: ::mapping::Project = serde_json::from_str(include_str!(
            "../../../codegen/src/tests/fixtures/static_primary_dynamic_named_xml_documents.json"
        ))
        .unwrap();
        codegen::lower(&project).unwrap()
    }

    #[test]
    fn mixed_mode_exports_four_owned_primary_and_named_document_apis() {
        let program = mixed_program();
        assert_eq!(
            program.xml_output_mode().unwrap(),
            Some(codegen::XmlOutputMode::StaticPrimaryDynamicNamedDocuments)
        );
        let source = super::super::render(&program).unwrap();
        let functions: Vec<_> = source
            .lines()
            .filter_map(|line| {
                line.strip_prefix("pub fn ")
                    .map(|rest| rest.split('(').next().unwrap())
            })
            .collect();
        assert_eq!(
            functions,
            [
                "execute_xml_document_outputs",
                "execute_xml_document_outputs_with_context",
                "execute_xml_bytes_document_outputs",
                "execute_xml_bytes_document_outputs_with_context",
            ]
        );
        assert!(source.contains("pub struct XmlDocumentExecutionOutputs { pub primary: String, pub extras: Vec<NamedXmlDocumentOutputs> }"));
        assert!(source.contains("pub struct XmlBytesDocumentExecutionOutputs { pub primary: Vec<u8>, pub extras: Vec<NamedXmlBytesDocumentOutputs> }"));
        assert!(source.contains("pub struct NamedXmlDocumentOutputs { pub name: &'static str, pub documents: Vec<XmlDocumentOutput> }"));
        assert!(source.contains("pub struct NamedXmlBytesDocumentOutputs { pub name: &'static str, pub documents: Vec<XmlBytesDocumentOutput> }"));
        assert!(!source.contains("pub fn execute_xml_outputs("));
        assert!(!source.contains("pub fn execute_xml_documents("));
    }

    #[test]
    fn invalid_mixed_policy_returns_typed_error_before_xml_source_exists() {
        let mut program = mixed_program();
        let policy = program.xml_boundary.as_mut().unwrap();
        policy.extra_outputs.push(codegen::NamedXmlOutputPolicy {
            name: "undeclared".into(),
            output: policy.output.clone(),
        });
        assert!(matches!(
            super::super::render(&program),
            Err(EmitError::InvalidProgram(_))
        ));
    }
}
