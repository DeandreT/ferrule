use super::output_arguments;
use crate::{EmitError, literal};
use codegen::{Program, ProgramValidationError};

pub(super) const TYPES: &str = "public sealed record XmlDocumentOutput(string Path, string Document);\npublic sealed record XmlBytesDocumentOutput(string Path, byte[] Document);\npublic sealed record NamedXmlDocumentOutputs(string Name, global::System.Collections.Generic.IReadOnlyList<XmlDocumentOutput> Documents);\npublic sealed record NamedXmlBytesDocumentOutputs(string Name, global::System.Collections.Generic.IReadOnlyList<XmlBytesDocumentOutput> Documents);\npublic sealed record XmlDocumentExecutionOutputs(string Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlDocumentOutputs> Extras);\npublic sealed record XmlBytesDocumentExecutionOutputs(byte[] Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlBytesDocumentOutputs> Extras);\n\n";

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
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
    output.push_str(&format!(
        "\n    private const string SourceXmlSchema = {};\n    private const string TargetXmlSchema = {};\n    private const string NamedXmlDocumentSchema = {};\n    private const string NamedXmlDocumentOutputName = {};\n",
        literal::string(&source),
        literal::string(&primary),
        literal::string(&target),
        literal::string(&named.name),
    ));
    output.push_str("    private static readonly global::System.Text.UTF8Encoding XmlDocumentOutputsUtf8 = new(false, true);\n");
    let primary_arguments = output_arguments(&policy.output)?;
    let named_arguments = output_arguments(&named_policy.output)?;
    for bytes in [false, true] {
        let name = if bytes {
            "ExecuteXmlBytesDocumentOutputs"
        } else {
            "ExecuteXmlDocumentOutputs"
        };
        let source_type = if bytes { "byte[]" } else { "string" };
        let parser = if bytes {
            "ParseStructuredEmbeddedBytes"
        } else {
            "ParseStructuredEmbedded"
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
            "SerializeXmlBytesDocumentOutputs"
        } else {
            "SerializeXmlDocumentOutputs"
        };
        let conversion = if bytes {
            "XmlDocumentOutputsUtf8.GetBytes(xml)"
        } else {
            "xml"
        };
        let primary_conversion = if bytes {
            "XmlDocumentOutputsUtf8.GetBytes(primaryXml)"
        } else {
            "primaryXml"
        };
        for context in [false, true] {
            let context_argument = if context {
                ", global::Ferrule.Runtime.FerruleExecutionContext executionContext"
            } else {
                ""
            };
            let context_call = if context { ", executionContext" } else { "" };
            output.push_str(&format!(r#"
    public static {result} {name}({source_type} source{context_argument})
    {{
        global::Ferrule.Runtime.FerruleInstance parsed;
        ExecutionOutputs mapped;
        try
        {{
            parsed = global::Ferrule.Runtime.FerruleXml.{parser}(SourceXmlSchema, source);
            try {{ mapped = ExecuteOutputs(parsed{context_call}); }}
            catch (global::Ferrule.Runtime.FerruleRuntimeException error)
            {{
                throw new global::Ferrule.Runtime.FerruleXmlBoundaryException(
                    global::Ferrule.Runtime.FerruleXmlBoundaryErrorKind.Mapping, error.Message, error);
            }}
        }}
        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.FromBoundary(error); }}
        return {helper}(mapped);
    }}
"#));
        }
        output.push_str(&format!(r#"
    private static {result} {helper}(ExecutionOutputs mapped)
    {{
        if (mapped.Primary is not global::Ferrule.Runtime.FerruleGroup)
            throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.Alignment("XML document outputs require an ordinary primary group");
        if (mapped.Extras.Count != 1 || mapped.Extras[0].Name != NamedXmlDocumentOutputName)
            throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.Alignment("XML document outputs do not match the declared named target");
        if (mapped.Extras[0].Instance is not global::Ferrule.Runtime.FerruleDocumentSet members)
            throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.Alignment("XML named document outputs require a document set");
        if (members.Documents.Count == global::System.Int32.MaxValue)
            throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.Alignment("XML document output artifact count exceeds the host index range");
        var artifactCount = checked(members.Documents.Count + 1);
        var budget = new global::Ferrule.Runtime.FerruleXmlDocumentOutputsBudget(artifactCount);
        string primaryXml;
        try {{ primaryXml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(TargetXmlSchema, mapped.Primary, {primary_arguments}); }}
        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.PrimarySerialization(error); }}
        budget.ChargePrimary(XmlDocumentOutputsUtf8.GetByteCount(primaryXml));
        var primary = {primary_conversion};
        var documents = new global::System.Collections.Generic.List<{dto}>(members.Documents.Count);
        for (var index = 0; index < members.Documents.Count; index++)
        {{
            var member = members.Documents[index];
            string xml;
            try {{ xml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(NamedXmlDocumentSchema, member.Value, {named_arguments}); }}
            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
            {{ throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.NamedSerialization(0, NamedXmlDocumentOutputName, index, member.Path, error); }}
            budget.ChargeNamed(0, NamedXmlDocumentOutputName, index, member.Path, XmlDocumentOutputsUtf8.GetByteCount(xml));
            documents.Add(new {dto}(member.Path, {conversion}));
        }}
        var extras = new global::System.Collections.Generic.List<{named_dto}>(1)
        {{
            new {named_dto}(NamedXmlDocumentOutputName, documents.AsReadOnly()),
        }};
        return new {result}(primary, extras.AsReadOnly());
    }}
"#));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixed_program() -> Program {
        let project: ::mapping::Project = serde_json::from_str(include_str!(
            "../../../../codegen/src/tests/fixtures/static_primary_dynamic_named_xml_documents.json"
        ))
        .unwrap();
        codegen::lower(&project).unwrap()
    }

    #[test]
    fn mixed_mode_exports_four_owned_primary_and_readonly_named_document_overloads() {
        let program = mixed_program();
        assert_eq!(
            program.xml_output_mode().unwrap(),
            Some(codegen::XmlOutputMode::StaticPrimaryDynamicNamedDocuments)
        );
        assert_eq!(super::super::render_types(&program).unwrap(), TYPES);
        let mut source = String::new();
        super::super::render(&program, &mut source).unwrap();
        let methods: Vec<_> = source
            .lines()
            .filter(|line| line.trim_start().starts_with("public static "))
            .collect();
        assert_eq!(methods.len(), 4);
        assert_eq!(
            methods
                .iter()
                .filter(|line| line.contains(" ExecuteXmlDocumentOutputs("))
                .count(),
            2
        );
        assert_eq!(
            methods
                .iter()
                .filter(|line| line.contains(" ExecuteXmlBytesDocumentOutputs("))
                .count(),
            2
        );
        assert!(!source.contains(" ExecuteXmlOutputs("));
        assert!(!source.contains(" ExecuteXmlDocuments("));
    }

    #[test]
    fn invalid_mixed_policy_returns_typed_error_before_xml_types_or_methods_exist() {
        let mut program = mixed_program();
        let policy = program.xml_boundary.as_mut().unwrap();
        policy.extra_outputs.push(codegen::NamedXmlOutputPolicy {
            name: "undeclared".into(),
            output: policy.output.clone(),
        });
        let mut source = String::new();
        assert!(matches!(
            super::super::render(&program, &mut source),
            Err(EmitError::ProgramValidation(_))
        ));
        assert!(source.is_empty());
        assert!(matches!(
            super::super::render_types(&program),
            Err(EmitError::ProgramValidation(_))
        ));
    }
}
