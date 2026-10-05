//! Static input admission plus dynamic-primary XML member serialization.
use super::output_arguments;
use crate::{EmitError, literal};
use codegen::{Program, ProgramValidationError, XmlOutputMode};

pub(super) const TYPES: &str = "public sealed record NamedXmlInput(string Name, string Document);\npublic sealed record NamedXmlBytesInput(string Name, byte[] Document);\npublic sealed record XmlDocumentOutput(string Path, string Document);\npublic sealed record XmlBytesDocumentOutput(string Path, byte[] Document);\n\n";

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
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
    output.push_str(&format!("\n    private const string SourceXmlSchema = {};\n    private const string TargetXmlSchema = {};\n", literal::string(&source), literal::string(&target)));
    let names = program
        .extra_sources
        .iter()
        .map(|source| literal::string(&source.name))
        .collect::<Vec<_>>()
        .join(", ");
    output.push_str(&format!(
        "    private static readonly string[] ExtraXmlInputNames = new string[] {{ {names} }};\n"
    ));
    for (index, source) in program.extra_sources.iter().enumerate() {
        let descriptor = codegen::serialize_embedded_schema(
            &source.source,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )?;
        output.push_str(&format!(
            "    private const string ExtraXmlInputSchema_{index} = {};\n",
            literal::string(&descriptor)
        ));
    }
    output.push_str("    private static readonly global::System.Text.UTF8Encoding XmlDocumentWithInputsUtf8 = new(false, true);\n");
    output.push_str(r#"
    // Only sealed static-admission calls enter these conversions. Host list
    // accessors and ordinary CLR shape guards remain outside this channel.
    private static T XmlDocumentInputAdmissionValue<T>(global::System.Func<T> action)
    {
        try { return action(); }
        catch (global::Ferrule.Runtime.FerruleXmlExecutionException error)
        { throw global::Ferrule.Runtime.FerruleXmlInputDocumentExecutionException.FromInputBoundary(error.Input, error.Boundary); }
    }
    private static void XmlDocumentInputAdmission(global::System.Action action)
    {
        try { action(); }
        catch (global::Ferrule.Runtime.FerruleXmlExecutionException error)
        { throw global::Ferrule.Runtime.FerruleXmlInputDocumentExecutionException.FromInputBoundary(error.Input, error.Boundary); }
    }
"#);
    let arguments = output_arguments(&policy.output)?;
    for bytes in [false, true] {
        let method = if bytes {
            "ExecuteXmlBytesDocumentsWithSources"
        } else {
            "ExecuteXmlDocumentsWithSources"
        };
        let input = if bytes {
            "NamedXmlBytesInput"
        } else {
            "NamedXmlInput"
        };
        let ty = if bytes { "byte[]" } else { "string" };
        let dto = if bytes {
            "XmlBytesDocumentOutput"
        } else {
            "XmlDocumentOutput"
        };
        let parser = if bytes {
            "ParseStructuredEmbeddedBytes"
        } else {
            "ParseStructuredEmbedded"
        };
        let parse_helper = if bytes {
            "ParseXmlBytesDocumentInputs"
        } else {
            "ParseXmlDocumentInputs"
        };
        let serialize_helper = if bytes {
            "SerializeXmlBytesDocumentMembersWithInputs"
        } else {
            "SerializeXmlDocumentMembersWithInputs"
        };
        for context in [false, true] {
            let context_arg = if context {
                ", global::Ferrule.Runtime.FerruleExecutionContext executionContext"
            } else {
                ""
            };
            let context_call = if context { ", executionContext" } else { "" };
            let require_context = if context {
                "        global::System.ArgumentNullException.ThrowIfNull(executionContext);\n"
            } else {
                ""
            };
            output.push_str(&format!(r#"
    public static global::System.Collections.Generic.IReadOnlyList<{dto}> {method}(
        {ty} source, global::System.Collections.Generic.IReadOnlyList<{input}> extraSources{context_arg})
    {{
{require_context}        var parsed = {parse_helper}(source, extraSources);
        ExecutionOutputs mapped;
        try {{ mapped = ExecuteOutputsWithSources(parsed.Primary, parsed.Inputs{context_call}); }}
        catch (global::Ferrule.Runtime.FerruleRuntimeException error)
        {{
            throw global::Ferrule.Runtime.FerruleXmlInputDocumentExecutionException.FromBoundary(
                new global::Ferrule.Runtime.FerruleXmlBoundaryException(
                    global::Ferrule.Runtime.FerruleXmlBoundaryErrorKind.Mapping, error.Message, error));
        }}
        return {serialize_helper}(mapped);
    }}
"#));
        }
        output.push_str(&format!(r#"
    private static (global::Ferrule.Runtime.FerruleInstance Primary,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> Inputs) {parse_helper}(
        {ty} source, global::System.Collections.Generic.IReadOnlyList<{input}> extraSources)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
            var suppliedCount = extraSources.Count;
            var budget = XmlDocumentInputAdmissionValue<global::Ferrule.Runtime.FerruleXmlInputSetBudget>(
                () => new global::Ferrule.Runtime.FerruleXmlInputSetBudget((long)suppliedCount + 1));
            var suppliedNames = new string[suppliedCount];
            for (var i = 0; i < suppliedCount; i++)
            {{
                global::System.ArgumentNullException.ThrowIfNull(extraSources[i]);
                global::System.ArgumentNullException.ThrowIfNull(extraSources[i].Name);
                suppliedNames[i] = extraSources[i].Name;
            }}
            var indices = XmlDocumentInputAdmissionValue<int[]>(() =>
                global::Ferrule.Runtime.FerruleXmlInputSetBudget.Indices(ExtraXmlInputNames, suppliedNames));
            // Complete names precede complete Document shape, then all sizes.
            foreach (var input in extraSources)
                global::System.ArgumentNullException.ThrowIfNull(input.Document);
            var primaryOwner = global::Ferrule.Runtime.FerruleXmlInputSource.Primary;
"#));
        let primary_size = if bytes {
            "source.LongLength"
        } else {
            "XmlDocumentInputAdmissionValue<long>(() => global::Ferrule.Runtime.FerruleXmlInputSetBudget.Measure(primaryOwner, source))"
        };
        output.push_str(&format!("            var primaryBytes = {primary_size};\n            XmlDocumentInputAdmission(() => {{ global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(primaryOwner, primaryBytes); }});\n"));
        for (index, source) in program.extra_sources.iter().enumerate() {
            let name = literal::string(&source.name);
            output.push_str(&format!("            var owner_{index} = global::Ferrule.Runtime.FerruleXmlInputSource.Named({index}, {name});\n            var input_{index} = extraSources[indices[{index}]].Document;\n            global::System.ArgumentNullException.ThrowIfNull(input_{index});\n"));
            let size = if bytes {
                format!("input_{index}.LongLength")
            } else {
                format!(
                    "XmlDocumentInputAdmissionValue<long>(() => global::Ferrule.Runtime.FerruleXmlInputSetBudget.Measure(owner_{index}, input_{index}))"
                )
            };
            output.push_str(&format!("            var bytes_{index} = {size};\n            XmlDocumentInputAdmission(() => {{ global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(owner_{index}, bytes_{index}); }});\n"));
        }
        output.push_str("            XmlDocumentInputAdmission(() => { budget.Charge(primaryOwner, primaryBytes); });\n");
        for index in 0..program.extra_sources.len() {
            output.push_str(&format!("            XmlDocumentInputAdmission(() => {{ budget.Charge(owner_{index}, bytes_{index}); }});\n"));
        }
        output.push_str(&format!("            global::Ferrule.Runtime.FerruleInstance primary;\n            try {{ primary = global::Ferrule.Runtime.FerruleXml.{parser}(SourceXmlSchema, source); }}\n            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)\n            {{ throw global::Ferrule.Runtime.FerruleXmlInputDocumentExecutionException.FromInputBoundary(primaryOwner, error); }}\n            var parsedInputs = new global::System.Collections.Generic.List<NamedInput>({});\n", program.extra_sources.len()));
        for (index, source) in program.extra_sources.iter().enumerate() {
            output.push_str(&format!("            try {{ parsedInputs.Add(new NamedInput({}, global::Ferrule.Runtime.FerruleXml.{parser}(ExtraXmlInputSchema_{index}, input_{index}))); }}\n            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)\n            {{ throw global::Ferrule.Runtime.FerruleXmlInputDocumentExecutionException.FromInputBoundary(owner_{index}, error); }}\n", literal::string(&source.name)));
        }
        output.push_str(
            r#"            return (primary, parsedInputs);
    }
"#,
        );
        output.push_str(&format!(r#"
    private static global::System.Collections.Generic.IReadOnlyList<{dto}> {serialize_helper}(ExecutionOutputs mapped)
    {{
        if (mapped.Extras.Count != 0)
            throw global::Ferrule.Runtime.FerruleXmlInputDocumentExecutionException.FromDocuments(
                global::Ferrule.Runtime.FerruleXmlDocumentExecutionException.Alignment("dynamic XML documents require no named mapped outputs"));
        if (mapped.Primary is not global::Ferrule.Runtime.FerruleDocumentSet members)
            throw global::Ferrule.Runtime.FerruleXmlInputDocumentExecutionException.FromDocuments(
                global::Ferrule.Runtime.FerruleXmlDocumentExecutionException.Alignment("dynamic XML documents require a primary document set"));
        global::Ferrule.Runtime.FerruleXmlDocumentSetBudget budget;
        try {{ budget = new global::Ferrule.Runtime.FerruleXmlDocumentSetBudget(members.Documents.Count); }}
        catch (global::Ferrule.Runtime.FerruleXmlDocumentExecutionException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlInputDocumentExecutionException.FromDocuments(error); }}
        var outputs = new global::System.Collections.Generic.List<{dto}>(members.Documents.Count);
        for (var index = 0; index < members.Documents.Count; index++)
        {{
            var member = members.Documents[index];
            string xml;
            try {{ xml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(TargetXmlSchema, member.Value, {arguments}); }}
            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
            {{
                throw global::Ferrule.Runtime.FerruleXmlInputDocumentExecutionException.FromDocuments(
                    global::Ferrule.Runtime.FerruleXmlDocumentExecutionException.Serialization(index, member.Path, error));
            }}
            try {{ budget.Charge(index, member.Path, XmlDocumentWithInputsUtf8.GetByteCount(xml)); }}
            catch (global::Ferrule.Runtime.FerruleXmlDocumentExecutionException error)
            {{ throw global::Ferrule.Runtime.FerruleXmlInputDocumentExecutionException.FromDocuments(error); }}
            outputs.Add(new {dto}(member.Path, {conversion}));
        }}
        return outputs.AsReadOnly();
    }}
"#, conversion = if bytes { "XmlDocumentWithInputsUtf8.GetBytes(xml)" } else { "xml" }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn program() -> Program {
        let project: ::mapping::Project = serde_json::from_str(include_str!("../../../../codegen/src/tests/fixtures/static_named_inputs_dynamic_primary_xml_documents.json")).unwrap();
        codegen::lower(&project).unwrap()
    }
    #[test]
    fn emits_four_source_overloads_and_both_exact_input_schemas() {
        let mut output = String::new();
        render(&program(), &mut output).unwrap();
        assert_eq!(
            output
                .matches("public static global::System.Collections.Generic.IReadOnlyList<")
                .count(),
            4
        );
        assert_eq!(output.matches("ExecuteXmlDocumentsWithSources(").count(), 2);
        assert_eq!(
            output
                .matches("ExecuteXmlBytesDocumentsWithSources(")
                .count(),
            2
        );
        assert!(output.contains("FerruleXmlInputSource.Named(1, \"beta\")"));
        assert!(output.contains("ExtraXmlInputSchema_0"));
        assert!(output.contains("ExtraXmlInputSchema_1"));
        assert!(output.contains("outputs.AsReadOnly()"));
        assert!(!output.contains("ExecuteXmlDocuments("));
        assert!(!output.contains("ExecuteXmlOutputs("));
        assert!(!output.contains("FerruleXmlDynamicSourceAdapter"));
        assert_eq!(super::super::render_types(&program()).unwrap(), TYPES);
    }
    #[test]
    fn invalid_later_schema_policy_fails_before_types_or_rendering() {
        let mut invalid = program();
        invalid.xml_boundary.as_mut().unwrap().extra_inputs[1].name = "wrong".into();
        assert!(super::super::render_types(&invalid).is_err());
        let mut source = String::new();
        assert!(render(&invalid, &mut source).is_err());
        assert!(source.is_empty());
        invalid.xml_boundary = None;
        assert_eq!(super::super::render_types(&invalid).unwrap(), "");
        super::super::render(&invalid, &mut source).unwrap();
        assert!(source.is_empty());
    }
}
