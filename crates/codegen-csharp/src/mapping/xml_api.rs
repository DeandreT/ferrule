mod dynamic_input_document_sets;
mod dynamic_inputs;
mod input_document_outputs;
mod input_document_sets;
mod named_document_outputs;
mod named_inputs;

use crate::{EmitError, literal};
use codegen::{Program, ProgramValidationError, XmlInputProfile, XmlOutputPolicy};

pub(super) fn render_types(program: &Program) -> Result<&'static str, EmitError> {
    if program.xml_output_mode()?
        == Some(codegen::XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments)
    {
        return Ok(dynamic_input_document_sets::TYPES);
    }
    if program.xml_output_mode()?
        == Some(codegen::XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments)
    {
        return Ok(input_document_outputs::TYPES);
    }
    if program.xml_output_mode()?
        == Some(codegen::XmlOutputMode::StaticNamedInputsDynamicPrimaryDocuments)
    {
        return Ok(input_document_sets::TYPES);
    }
    if program.xml_output_mode()? == Some(codegen::XmlOutputMode::DynamicPrimaryDocuments) {
        return Ok(
            "public sealed record XmlDocumentOutput(string Path, string Document);\npublic sealed record XmlBytesDocumentOutput(string Path, byte[] Document);\n\n",
        );
    }
    if program.xml_output_mode()?
        == Some(codegen::XmlOutputMode::StaticPrimaryDynamicNamedDocuments)
    {
        return Ok(named_document_outputs::TYPES);
    }
    if program.xml_boundary.is_none() {
        return Ok("");
    }
    if program.xml_boundary.as_ref().is_some_and(|policy| {
        policy.input.profile() == Some(XmlInputProfile::Structured)
            || (policy.input.profile() == Some(XmlInputProfile::RootView)
                && !program.extra_sources.is_empty())
    }) {
        return Ok(named_inputs::TYPES);
    }
    Ok(
        "public sealed record NamedXmlOutput(string Name, string Document);\npublic sealed record NamedXmlBytesOutput(string Name, byte[] Document);\npublic sealed record XmlExecutionOutputs(string Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlOutput> Extras);\npublic sealed record XmlBytesExecutionOutputs(byte[] Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlBytesOutput> Extras);\n\n",
    )
}

fn output_arguments(policy: &XmlOutputPolicy) -> Result<String, EmitError> {
    let namespace = policy
        .default_namespace
        .as_ref()
        .map(|uri| literal::string(uri))
        .unwrap_or_else(|| "null".into());
    let hints = policy
        .schema_hints
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| EmitError::SchemaSerialization(error.to_string()))?
        .map(|json| literal::string(&json))
        .unwrap_or_else(|| "null".into());
    Ok(format!(
        "{}, {}, {namespace}, {hints}",
        policy.declaration, policy.indent
    ))
}

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
    if program.xml_output_mode()?
        == Some(codegen::XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments)
    {
        return dynamic_input_document_sets::render(program, output);
    }
    if program.xml_output_mode()?
        == Some(codegen::XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments)
    {
        return input_document_outputs::render(program, output);
    }
    if program.xml_output_mode()?
        == Some(codegen::XmlOutputMode::StaticNamedInputsDynamicPrimaryDocuments)
    {
        return input_document_sets::render(program, output);
    }
    if program.xml_output_mode()? == Some(codegen::XmlOutputMode::DynamicPrimaryDocuments) {
        return render_dynamic_documents(program, output);
    }
    if program.xml_output_mode()?
        == Some(codegen::XmlOutputMode::StaticPrimaryDynamicNamedDocuments)
    {
        return named_document_outputs::render(program, output);
    }
    let Some(policy) = &program.xml_boundary else {
        return Ok(());
    };
    let profile =
        policy
            .input
            .profile()
            .ok_or_else(|| ProgramValidationError::InvalidXmlBoundary {
                reason: "XML input policy has incompatible profile flags".into(),
            })?;
    let source = codegen::serialize_embedded_schema(
        &program.source,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let target = codegen::serialize_embedded_schema(
        &program.target,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    output.push_str(&format!("\n    private const string SourceXmlSchema = {};\n    private const string TargetXmlSchema = {};\n",literal::string(&source),literal::string(&target)));
    for (index, target) in program.extra_targets.iter().enumerate() {
        let descriptor = codegen::serialize_embedded_schema(
            &target.target,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?;
        output.push_str(&format!(
            "    private const string ExtraXmlSchema_{index} = {};\n",
            literal::string(&descriptor)
        ));
    }
    output.push_str("    private static readonly global::System.Text.UTF8Encoding XmlOutputUtf8 = new(false, true);\n");
    let named_input_route = profile == XmlInputProfile::Structured
        || (profile == XmlInputProfile::RootView && !program.extra_sources.is_empty());
    if named_input_route {
        named_inputs::render(program, output)?;
    }
    if profile == XmlInputProfile::Structured {
        dynamic_inputs::render(program, output);
    }
    for bytes in [false, true] {
        let name = if bytes {
            "ExecuteXmlBytesOutputs"
        } else {
            "ExecuteXmlOutputs"
        };
        let singular = if bytes {
            "ExecuteXmlBytes"
        } else {
            "ExecuteXml"
        };
        let ty = if bytes { "byte[]" } else { "string" };
        let result_type = if bytes {
            "XmlBytesExecutionOutputs"
        } else {
            "XmlExecutionOutputs"
        };
        let helper = if bytes {
            "SerializeXmlBytesOutputs"
        } else {
            "SerializeXmlOutputs"
        };
        for context in [false, true] {
            if named_input_route {
                continue;
            }
            let parser = match profile {
                XmlInputProfile::RootView => {
                    if bytes {
                        "ParseEmbeddedBytes"
                    } else {
                        "ParseEmbedded"
                    }
                }
                XmlInputProfile::Structured => {
                    if bytes {
                        "ParseStructuredEmbeddedBytes"
                    } else {
                        "ParseStructuredEmbedded"
                    }
                }
            };
            let input_args = if profile == XmlInputProfile::RootView {
                format!(
                    ", {}, {}",
                    policy.input.allow_inactive_root_type_members, policy.input.root_view_policy
                )
            } else {
                String::new()
            };
            let context_arg = if context {
                ", global::Ferrule.Runtime.FerruleExecutionContext executionContext"
            } else {
                ""
            };
            let context_call = if context { ", executionContext" } else { "" };
            output.push_str(&format!(r#"
    public static {result_type} {name}({ty} source{context_arg})
    {{
        global::Ferrule.Runtime.FerruleInstance parsed;
        ExecutionOutputs mapped;
        try
        {{
            parsed = global::Ferrule.Runtime.FerruleXml.{parser}(SourceXmlSchema, source{input_args});
            try {{ mapped = ExecuteOutputs(parsed{context_call}); }}
            catch (global::Ferrule.Runtime.FerruleRuntimeException error)
            {{
                throw new global::Ferrule.Runtime.FerruleXmlBoundaryException(
                    global::Ferrule.Runtime.FerruleXmlBoundaryErrorKind.Mapping, error.Message, error);
            }}
        }}
        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
        {{ throw new global::Ferrule.Runtime.FerruleXmlOutputSetException(null, error); }}
        return {helper}(mapped);
    }}

    public static {ty} {singular}({ty} source{context_arg})
    {{
        try {{ return {name}(source{context_call}).Primary; }}
        catch (global::Ferrule.Runtime.FerruleXmlOutputSetException error) {{ throw error.Boundary; }}
    }}
"#));
        }
        let mut alignment = format!("mapped.Extras.Count != {}", program.extra_targets.len());
        for (index, target) in program.extra_targets.iter().enumerate() {
            alignment.push_str(&format!(
                " || mapped.Extras[{index}].Name != {}",
                literal::string(&target.name)
            ));
        }
        output.push_str(&format!("\n    private static {result_type} {helper}(ExecutionOutputs mapped)\n    {{\n        if ({alignment})\n            throw global::Ferrule.Runtime.FerruleXmlOutputSetBudget.Alignment(\"XML mapped outputs do not match declared target names/order\");\n        var budget = new global::Ferrule.Runtime.FerruleXmlOutputSetBudget(mapped.Extras.Count + 1);\n"));
        let args = output_arguments(&policy.output)?;
        output.push_str(&format!("        var primaryTarget = global::Ferrule.Runtime.FerruleXmlOutputTarget.Primary;\n        var primaryXml = SerializeXmlTarget(primaryTarget, TargetXmlSchema, mapped.Primary, {args});\n        budget.Charge(primaryTarget, XmlOutputUtf8.GetByteCount(primaryXml));\n        var primary = {};\n", if bytes { "XmlOutputUtf8.GetBytes(primaryXml)" } else { "primaryXml" }));
        let dto = if bytes {
            "NamedXmlBytesOutput"
        } else {
            "NamedXmlOutput"
        };
        output.push_str(&format!(
            "        var extras = new global::System.Collections.Generic.List<{dto}>({});\n",
            program.extra_targets.len()
        ));
        for (index, (target, policy)) in program
            .extra_targets
            .iter()
            .zip(&policy.extra_outputs)
            .enumerate()
        {
            let name = literal::string(&target.name);
            let args = output_arguments(&policy.output)?;
            output.push_str(&format!("        var target_{index} = global::Ferrule.Runtime.FerruleXmlOutputTarget.Named({index}, {name});\n        var xml_{index} = SerializeXmlTarget(target_{index}, ExtraXmlSchema_{index}, mapped.Extras[{index}].Instance, {args});\n        budget.Charge(target_{index}, XmlOutputUtf8.GetByteCount(xml_{index}));\n        extras.Add(new {dto}({name}, {}));\n", if bytes { format!("XmlOutputUtf8.GetBytes(xml_{index})") } else { format!("xml_{index}") }));
        }
        output.push_str(&format!(
            "        return new {result_type}(primary, extras.AsReadOnly());\n    }}\n"
        ));
    }
    output.push_str(
        r#"
    private static string SerializeXmlTarget(
        global::Ferrule.Runtime.FerruleXmlOutputTarget target,
        string descriptor, global::Ferrule.Runtime.FerruleInstance instance,
        bool declaration, bool indent, string? defaultNamespace, string? hints)
    {
        try { return global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(
            descriptor, instance, declaration, indent, defaultNamespace, hints); }
        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
        { throw new global::Ferrule.Runtime.FerruleXmlOutputSetException(target, error); }
    }
"#,
    );
    Ok(())
}

// SingleDocument keeps its existing renderer. Only the checked dynamic mode
// reaches these additive no-I/O document-list entry points.
fn render_dynamic_documents(program: &Program, output: &mut String) -> Result<(), EmitError> {
    let policy = program.xml_boundary.as_ref().ok_or_else(|| {
        ProgramValidationError::InvalidXmlBoundary {
            reason: "dynamic XML document mode requires a boundary".into(),
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
    output.push_str(&format!("\n    private const string SourceXmlSchema = {};\n    private const string TargetXmlSchema = {};\n", literal::string(&source), literal::string(&target)));
    output.push_str("    private static readonly global::System.Text.UTF8Encoding XmlDocumentOutputUtf8 = new(false, true);\n");
    let arguments = output_arguments(&policy.output)?;
    for bytes in [false, true] {
        let name = if bytes {
            "ExecuteXmlBytesDocuments"
        } else {
            "ExecuteXmlDocuments"
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
        let helper = if bytes {
            "SerializeXmlBytesDocumentMembers"
        } else {
            "SerializeXmlDocumentMembers"
        };
        for context in [false, true] {
            let context_arg = if context {
                ", global::Ferrule.Runtime.FerruleExecutionContext executionContext"
            } else {
                ""
            };
            let context_call = if context { ", executionContext" } else { "" };
            output.push_str(&format!(r#"
    public static global::System.Collections.Generic.IReadOnlyList<{dto}> {name}({source_type} source{context_arg})
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
        {{ throw global::Ferrule.Runtime.FerruleXmlDocumentExecutionException.FromBoundary(error); }}
        return {helper}(mapped);
    }}
"#));
        }
        output.push_str(&format!(r#"
    private static global::System.Collections.Generic.IReadOnlyList<{dto}> {helper}(ExecutionOutputs mapped)
    {{
        if (mapped.Extras.Count != 0)
            throw global::Ferrule.Runtime.FerruleXmlDocumentExecutionException.Alignment("dynamic XML documents require no named mapped outputs");
        if (mapped.Primary is not global::Ferrule.Runtime.FerruleDocumentSet members)
            throw global::Ferrule.Runtime.FerruleXmlDocumentExecutionException.Alignment("dynamic XML documents require a primary document set");
        var budget = new global::Ferrule.Runtime.FerruleXmlDocumentSetBudget(members.Documents.Count);
        var outputs = new global::System.Collections.Generic.List<{dto}>(members.Documents.Count);
        for (var index = 0; index < members.Documents.Count; index++)
        {{
            var member = members.Documents[index];
            string xml;
            try {{ xml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(TargetXmlSchema, member.Value, {arguments}); }}
            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
            {{ throw global::Ferrule.Runtime.FerruleXmlDocumentExecutionException.Serialization(index, member.Path, error); }}
            budget.Charge(index, member.Path, XmlDocumentOutputUtf8.GetByteCount(xml));
            outputs.Add(new {dto}(member.Path, {conversion}));
        }}
        return outputs.AsReadOnly();
    }}
"#, conversion = if bytes { "XmlDocumentOutputUtf8.GetBytes(xml)" } else { "xml" }));
    }
    Ok(())
}

#[cfg(test)]
mod dynamic_document_tests {
    use super::*;

    fn dynamic_program() -> Program {
        let project: ::mapping::Project = serde_json::from_str(include_str!(
            "../../../codegen/src/tests/fixtures/dynamic_primary_xml_documents.json"
        ))
        .unwrap();
        codegen::lower(&project).unwrap()
    }

    #[test]
    fn dynamic_document_mode_emits_only_text_and_byte_list_overloads_with_owned_dtos() {
        let program = dynamic_program();
        assert_eq!(
            program.xml_output_mode().unwrap(),
            Some(codegen::XmlOutputMode::DynamicPrimaryDocuments)
        );
        assert_eq!(
            render_types(&program).unwrap(),
            "public sealed record XmlDocumentOutput(string Path, string Document);\npublic sealed record XmlBytesDocumentOutput(string Path, byte[] Document);\n\n"
        );
        let mut source = String::new();
        render(&program, &mut source).unwrap();
        let methods: Vec<_> = source
            .lines()
            .filter(|line| line.trim_start().starts_with("public static "))
            .collect();
        assert_eq!(methods.len(), 4);
        assert_eq!(
            methods
                .iter()
                .filter(|line| line.contains(" ExecuteXmlDocuments("))
                .count(),
            2
        );
        assert_eq!(
            methods
                .iter()
                .filter(|line| line.contains(" ExecuteXmlBytesDocuments("))
                .count(),
            2
        );
        assert!(!source.contains(" ExecuteXml("));
        assert!(!source.contains(" ExecuteXmlOutputs("));
    }

    #[test]
    fn invalid_hand_built_dynamic_boundary_refuses_before_rendering_while_core_only_emits_no_xml() {
        let mut program = dynamic_program();
        let policy = program.xml_boundary.as_mut().unwrap();
        policy.extra_outputs.push(codegen::NamedXmlOutputPolicy {
            name: "undeclared".into(),
            output: policy.output.clone(),
        });
        let mut source = String::new();
        assert!(matches!(
            render(&program, &mut source),
            Err(EmitError::ProgramValidation(_))
        ));
        assert!(source.is_empty());
        assert!(matches!(
            render_types(&program),
            Err(EmitError::ProgramValidation(_))
        ));
        program.xml_boundary = None;
        render(&program, &mut source).unwrap();
        assert!(source.is_empty());
        assert!(render_types(&program).unwrap().is_empty());
    }
}
