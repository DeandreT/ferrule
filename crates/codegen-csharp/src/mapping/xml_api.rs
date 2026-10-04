use crate::{EmitError, literal};
use codegen::{Program, ProgramValidationError, XmlInputProfile, XmlOutputPolicy};

pub(super) fn render_types(program: &Program) -> &'static str {
    if program.xml_boundary.is_none() {
        return "";
    }
    "public sealed record NamedXmlOutput(string Name, string Document);\npublic sealed record NamedXmlBytesOutput(string Name, byte[] Document);\npublic sealed record XmlExecutionOutputs(string Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlOutput> Extras);\npublic sealed record XmlBytesExecutionOutputs(byte[] Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlBytesOutput> Extras);\n\n"
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
