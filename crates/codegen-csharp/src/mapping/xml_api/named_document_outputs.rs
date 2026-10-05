use super::output_arguments;
use crate::{EmitError, literal};
use codegen::{Program, ProgramValidationError};

pub(super) const TYPES: &str = "public sealed record XmlDocumentOutput(string Path, string Document);\npublic sealed record XmlBytesDocumentOutput(string Path, byte[] Document);\npublic sealed record NamedXmlDocumentOutputs(string Name, global::System.Collections.Generic.IReadOnlyList<XmlDocumentOutput> Documents);\npublic sealed record NamedXmlBytesDocumentOutputs(string Name, global::System.Collections.Generic.IReadOnlyList<XmlBytesDocumentOutput> Documents);\npublic sealed record XmlDocumentExecutionOutputs(string Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlDocumentOutputs> Extras);\npublic sealed record XmlBytesDocumentExecutionOutputs(byte[] Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlBytesDocumentOutputs> Extras);\n\n";

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
    if program.extra_targets.len() > 1 {
        return render_multiple(program, output);
    }
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

fn render_multiple(program: &Program, output: &mut String) -> Result<(), EmitError> {
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
    output.push_str(&format!(
        "\n    private const string SourceXmlSchema = {};\n    private const string TargetXmlSchema = {};\n",
        literal::string(&source), literal::string(&primary),
    ));
    for (declaration_index, target) in program.extra_targets.iter().enumerate() {
        let descriptor = codegen::serialize_embedded_schema(
            &target.target,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?;
        output.push_str(&format!(
            "    private const string NamedXmlDocumentSchema{declaration_index} = {};\n    private const string NamedXmlDocumentOutputName{declaration_index} = {};\n",
            literal::string(&descriptor), literal::string(&target.name),
        ));
    }
    output.push_str("    private static readonly global::System.Text.UTF8Encoding XmlDocumentOutputsUtf8 = new(false, true);\n");
    let primary_arguments = output_arguments(&policy.output)?;
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
        let target_count = program.extra_targets.len();
        output.push_str(&format!(r#"
    private static {result} {helper}(ExecutionOutputs mapped)
    {{
        if (mapped.Primary is not global::Ferrule.Runtime.FerruleGroup)
            throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.Alignment("XML document outputs require an ordinary primary group");
        if (mapped.Extras.Count != {target_count})
            throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.Alignment("XML document outputs do not match the declared named targets");
"#));
        // Align every named envelope before checking any total or serializing.
        for declaration_index in 0..target_count {
            output.push_str(&format!(r#"        if (mapped.Extras[{declaration_index}].Name != NamedXmlDocumentOutputName{declaration_index})
            throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.Alignment("XML document outputs do not match exact named declaration order");
        if (mapped.Extras[{declaration_index}].Instance is not global::Ferrule.Runtime.FerruleDocumentSet members{declaration_index})
            throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.Alignment("XML named document outputs require document sets");
"#));
        }
        output.push_str("        var artifactCount = 1;\n");
        for declaration_index in 0..target_count {
            output.push_str(&format!(r#"        if (members{declaration_index}.Documents.Count > global::System.Int32.MaxValue - artifactCount)
            throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.Alignment("XML document output artifact count exceeds the host index range");
        artifactCount = checked(artifactCount + members{declaration_index}.Documents.Count);
"#));
        }
        output.push_str(&format!(r#"        var budget = new global::Ferrule.Runtime.FerruleXmlDocumentOutputsBudget(artifactCount);
        string primaryXml;
        try {{ primaryXml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(TargetXmlSchema, mapped.Primary, {primary_arguments}); }}
        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.PrimarySerialization(error); }}
        budget.ChargePrimary(XmlDocumentOutputsUtf8.GetByteCount(primaryXml));
        var primary = {primary_conversion};
        var extras = new global::System.Collections.Generic.List<{named_dto}>({target_count});
"#));
        for (declaration_index, named_policy) in policy.extra_outputs.iter().enumerate() {
            let named_arguments = output_arguments(&named_policy.output)?;
            output.push_str(&format!(r#"        var documents{declaration_index} = new global::System.Collections.Generic.List<{dto}>(members{declaration_index}.Documents.Count);
        for (var index = 0; index < members{declaration_index}.Documents.Count; index++)
        {{
            var member = members{declaration_index}.Documents[index];
            string xml;
            try {{ xml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(NamedXmlDocumentSchema{declaration_index}, member.Value, {named_arguments}); }}
            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
            {{ throw global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException.NamedSerialization({declaration_index}, NamedXmlDocumentOutputName{declaration_index}, index, member.Path, error); }}
            budget.ChargeNamed({declaration_index}, NamedXmlDocumentOutputName{declaration_index}, index, member.Path, XmlDocumentOutputsUtf8.GetByteCount(xml));
            documents{declaration_index}.Add(new {dto}(member.Path, {conversion}));
        }}
        extras.Add(new {named_dto}(NamedXmlDocumentOutputName{declaration_index}, documents{declaration_index}.AsReadOnly()));
"#));
        }
        output.push_str(&format!(
            "        return new {result}(primary, extras.AsReadOnly());\n    }}\n"
        ));
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

    fn plural_project() -> ::mapping::Project {
        serde_json::from_str(include_str!(
            "../../../../codegen/src/tests/fixtures/static_primary_multiple_dynamic_named_xml_documents.json"
        )).unwrap()
    }

    #[test]
    fn plural_xml_outputs_keep_each_declared_schema_policy_and_owner_after_reversal() {
        let mut project = plural_project();
        for reversed in [false, true] {
            if reversed {
                project.extra_targets.swap(0, 1);
            }
            let program = codegen::lower(&project).unwrap();
            assert_eq!(super::super::render_types(&program).unwrap(), TYPES);
            let mut source = String::new();
            super::super::render(&program, &mut source).unwrap();
            assert_eq!(
                source
                    .lines()
                    .filter(|line| line.trim_start().starts_with("public static "))
                    .count(),
                4
            );
            for (declaration_index, target) in program.extra_targets.iter().enumerate() {
                let descriptor = codegen::serialize_embedded_schema(
                    &target.target,
                    codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
                )
                .unwrap();
                assert!(source.contains(&format!(
                    "private const string NamedXmlDocumentSchema{declaration_index} = {};",
                    literal::string(&descriptor),
                )));
                assert!(source.contains(&format!(
                    "private const string NamedXmlDocumentOutputName{declaration_index} = {};",
                    literal::string(&target.name),
                )));
                let policy =
                    &program.xml_boundary.as_ref().unwrap().extra_outputs[declaration_index].output;
                assert!(source.contains(&format!(
                    "SerializeDocumentEmbedded(NamedXmlDocumentSchema{declaration_index}, member.Value, {})",
                    output_arguments(policy).unwrap(),
                )));
                assert!(source.contains(&format!(
                    "NamedSerialization({declaration_index}, NamedXmlDocumentOutputName{declaration_index}, index, member.Path, error)",
                )));
            }
            let second_alignment = source.find("FerruleDocumentSet members1").unwrap();
            let first_count = source
                .find("artifactCount = checked(artifactCount + members0.Documents.Count)")
                .unwrap();
            let last_count = source
                .find("artifactCount = checked(artifactCount + members1.Documents.Count)")
                .unwrap();
            let budget = source
                .find("FerruleXmlDocumentOutputsBudget(artifactCount)")
                .unwrap();
            let primary = source
                .find("SerializeDocumentEmbedded(TargetXmlSchema")
                .unwrap();
            assert!(
                second_alignment < first_count
                    && first_count < last_count
                    && last_count < budget
                    && budget < primary
            );
        }
    }

    #[test]
    fn malformed_second_plural_policy_returns_typed_error_before_types_or_methods() {
        let mut program = codegen::lower(&plural_project()).unwrap();
        program.xml_boundary.as_mut().unwrap().extra_outputs[1].name = "undeclared".into();
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
