use crate::{EmitError, literal};
use codegen::Program;

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
    let Some(policy) = &program.xml_boundary else {
        return Ok(());
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
        .map(|uri| literal::string(uri))
        .unwrap_or_else(|| "null".into());
    let hints = policy
        .output
        .schema_hints
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| EmitError::SchemaSerialization(error.to_string()))?
        .map(|json| literal::string(&json))
        .unwrap_or_else(|| "null".into());
    output.push_str(&format!("\n    private const string SourceXmlSchema = {};\n    private const string TargetXmlSchema = {};\n",literal::string(&source),literal::string(&target)));
    for bytes in [false, true] {
        for context in [false, true] {
            let name = if bytes {
                "ExecuteXmlBytes"
            } else {
                "ExecuteXml"
            };
            let ty = if bytes { "byte[]" } else { "string" };
            let parser = if bytes {
                "ParseEmbeddedBytes"
            } else {
                "ParseEmbedded"
            };
            let context_arg = if context {
                ", global::Ferrule.Runtime.FerruleExecutionContext executionContext"
            } else {
                ""
            };
            let context_call = if context { ", executionContext" } else { "" };
            let result = if bytes {
                "global::System.Text.Encoding.UTF8.GetBytes(xml)"
            } else {
                "xml"
            };
            output.push_str(&format!(
                r#"
    public static {ty} {name}({ty} source{context_arg})
    {{
        var parsed = global::Ferrule.Runtime.FerruleXml.{parser}(SourceXmlSchema, source, {}, {});
        global::Ferrule.Runtime.FerruleInstance mapped;
        try
        {{
            mapped = Execute(parsed{context_call});
        }}
        catch (global::Ferrule.Runtime.FerruleRuntimeException error)
        {{
            throw new global::Ferrule.Runtime.FerruleXmlBoundaryException(
                global::Ferrule.Runtime.FerruleXmlBoundaryErrorKind.Mapping, error.Message, error);
        }}
        var xml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(
            TargetXmlSchema, mapped, {}, {}, {namespace}, {hints});
        return {result};
    }}
"#,
                policy.input.allow_inactive_root_type_members,
                policy.input.root_view_policy,
                policy.output.declaration,
                policy.output.indent
            ));
        }
    }
    Ok(())
}
