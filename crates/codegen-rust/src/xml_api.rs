use codegen::Program;

use crate::{EmitError, rust_string};

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
    let namespace = policy
        .output
        .default_namespace
        .as_ref()
        .map(|uri| format!("Some({})", rust_string(uri)))
        .unwrap_or_else(|| "None".into());
    let hints = policy
        .output
        .schema_hints
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| EmitError::SchemaSerialization(error.to_string()))?
        .map(|json| format!("Some({})", rust_string(&json)))
        .unwrap_or_else(|| "None".into());
    let mut output = format!(
        "\nconst SOURCE_XML_SCHEMA: &str = {};\nconst TARGET_XML_SCHEMA: &str = {};\n\n",
        rust_string(&source),
        rust_string(&target)
    );
    for bytes in [false, true] {
        for context in [false, true] {
            let name = format!(
                "execute_xml{}{}",
                if bytes { "_bytes" } else { "" },
                if context { "_with_context" } else { "" }
            );
            let source_type = if bytes { "&[u8]" } else { "&str" };
            let result_type = if bytes { "Vec<u8>" } else { "String" };
            let (parser, input_arguments) = match policy.input.profile() {
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
            let context_arg = if context {
                ", execution: &ExecutionContext<'_>"
            } else {
                ""
            };
            let execution = if context {
                "execute_with_context(&parsed, execution)"
            } else {
                "execute(&parsed)"
            };
            let result = if bytes { "xml.into_bytes()" } else { "xml" };
            output.push_str(&format!(
                "pub fn {name}(source: {source_type}{context_arg}) -> Result<{result_type}, codegen_runtime::XmlBoundaryError> {{\n\
                 let parsed = codegen_runtime::{parser}(SOURCE_XML_SCHEMA, source{input_arguments})?;\n\
                 let mapped = {execution}?;\n\
                 let xml = codegen_runtime::serialize_xml_document(TARGET_XML_SCHEMA, &mapped, {}, {}, {namespace}, {hints})?;\n\
                 Ok({result})\n}}\n\n",
                policy.output.declaration, policy.output.indent));
        }
    }
    Ok(output)
}
