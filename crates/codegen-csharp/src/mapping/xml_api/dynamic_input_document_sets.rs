//! One-loader/static input admission plus dynamic-primary XML member serialization.
use super::output_arguments;
use crate::{EmitError, literal};
use codegen::{Program, ProgramValidationError, XmlOutputMode};

pub(super) const TYPES: &str = "public sealed record NamedXmlInput(string Name, string Document);\npublic sealed record NamedXmlBytesInput(string Name, byte[] Document);\npublic sealed record XmlDocumentOutput(string Path, string Document);\npublic sealed record XmlBytesDocumentOutput(string Path, byte[] Document);\n\n";

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
    if program.xml_output_mode()? != Some(XmlOutputMode::DynamicNamedInputDynamicPrimaryDocuments) {
        return Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "dynamic-input XML document lists require their checked adapter mode".into(),
        }
        .into());
    }
    let policy = program.xml_boundary.as_ref().ok_or_else(|| {
        ProgramValidationError::InvalidXmlBoundary {
            reason: "missing dynamic-input document-list XML boundary".into(),
        }
    })?;
    let static_sources = program
        .extra_sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.dynamic.is_none())
        .collect::<Vec<_>>();
    let (dynamic_index, dynamic_source) = program
        .extra_sources
        .iter()
        .enumerate()
        .find(|(_, source)| source.dynamic.is_some())
        .ok_or_else(|| ProgramValidationError::InvalidXmlBoundary {
            reason: "dynamic-input document lists require their dynamic declaration".into(),
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
        .filter(|source| source.dynamic.is_none())
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
    output.push_str("    private static readonly global::System.Text.UTF8Encoding XmlDynamicDocumentUtf8 = new(false, true);\n");
    output.push_str(r#"
    // Only sealed static-admission calls and synchronous adapter recovery enter
    // this channel. Host list accessors and ordinary CLR shape guards stay outside.
    private static global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentExecutionException DynamicXmlDocumentInputError(
        global::Ferrule.Runtime.FerruleXmlExecutionException error)
    {
        global::System.Diagnostics.Debug.Assert(error.Output is null);
        if (error.Request is { } request)
        {
            global::System.Diagnostics.Debug.Assert(error.Input == global::Ferrule.Runtime.FerruleXmlInputSource.Named(request.DeclarationIndex, request.Source));
            return global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentExecutionException.FromDynamicInputBoundary(request, error.Boundary);
        }
        return global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentExecutionException.FromInputBoundary(error.Input, error.Boundary);
    }
    private static T XmlDynamicDocumentInputAdmissionValue<T>(global::System.Func<T> action)
    {
        try { return action(); }
        catch (global::Ferrule.Runtime.FerruleXmlExecutionException error)
        { throw DynamicXmlDocumentInputError(error); }
    }
    private static void XmlDynamicDocumentInputAdmission(global::System.Action action)
    {
        try { action(); }
        catch (global::Ferrule.Runtime.FerruleXmlExecutionException error)
        { throw DynamicXmlDocumentInputError(error); }
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
            "ParseXmlBytesDynamicDocumentInputs"
        } else {
            "ParseXmlDynamicDocumentInputs"
        };
        let serialize_helper = if bytes {
            "SerializeXmlBytesDocumentMembersWithDynamicInput"
        } else {
            "SerializeXmlDocumentMembersWithDynamicInput"
        };
        for context in [false, true] {
            let method = format!(
                "{method}{}",
                if context {
                    "ContextAndDynamicSourceLoader"
                } else {
                    "AndDynamicSourceLoader"
                }
            );
            let execute = if context {
                "ExecuteOutputsWithSourcesContextAndDynamicSourceLoader"
            } else {
                "ExecuteOutputsWithSourcesAndDynamicSourceLoader"
            };
            let dynamic_name = literal::string(&dynamic_source.name);
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
        {ty} source, global::System.Collections.Generic.IReadOnlyList<{input}> extraSources{context_arg},
        global::Ferrule.Runtime.IFerruleDynamicXmlSourceLoader loader)
    {{
{require_context}        global::System.ArgumentNullException.ThrowIfNull(loader);
        var parsed = {parse_helper}(source, extraSources);
        var adapter = new global::Ferrule.Runtime.FerruleXmlDynamicSourceAdapter(loader,
            new global::Ferrule.Runtime.FerruleXmlDynamicSourcePolicy({dynamic_index}, {dynamic_name}, ExtraXmlInputSchema_{dynamic_index}), parsed.Budget);
        ExecutionOutputs mapped;
        try {{ mapped = {execute}(parsed.Primary, parsed.Inputs{context_call}, adapter); }}
        catch (global::Ferrule.Runtime.FerruleRuntimeException error)
        {{ throw DynamicXmlDocumentInputError(adapter.Recover(error)); }}
        return {serialize_helper}(mapped);
    }}
"#));
        }
        output.push_str(&format!(r#"
    private static (global::Ferrule.Runtime.FerruleInstance Primary,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> Inputs,
        global::Ferrule.Runtime.FerruleXmlInputSetBudget Budget) {parse_helper}(
        {ty} source, global::System.Collections.Generic.IReadOnlyList<{input}> extraSources)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
            var suppliedCount = extraSources.Count;
            var budget = XmlDynamicDocumentInputAdmissionValue<global::Ferrule.Runtime.FerruleXmlInputSetBudget>(
                () => new global::Ferrule.Runtime.FerruleXmlInputSetBudget((long)suppliedCount + 1));
            var suppliedNames = new string[suppliedCount];
            for (var i = 0; i < suppliedCount; i++)
            {{
                global::System.ArgumentNullException.ThrowIfNull(extraSources[i]);
                global::System.ArgumentNullException.ThrowIfNull(extraSources[i].Name);
                suppliedNames[i] = extraSources[i].Name;
            }}
            var indices = XmlDynamicDocumentInputAdmissionValue<int[]>(() =>
                global::Ferrule.Runtime.FerruleXmlInputSetBudget.Indices(ExtraXmlInputNames, suppliedNames));
            // Complete names precede complete Document shape, then all sizes.
            foreach (var input in extraSources)
                global::System.ArgumentNullException.ThrowIfNull(input.Document);
            var primaryOwner = global::Ferrule.Runtime.FerruleXmlInputSource.Primary;
"#));
        let primary_size = if bytes {
            "source.LongLength"
        } else {
            "XmlDynamicDocumentInputAdmissionValue<long>(() => global::Ferrule.Runtime.FerruleXmlInputSetBudget.Measure(primaryOwner, source))"
        };
        output.push_str(&format!("            var primaryBytes = {primary_size};\n            XmlDynamicDocumentInputAdmission(() => {{ global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(primaryOwner, primaryBytes); }});\n"));
        for (static_index, (index, source)) in static_sources.iter().enumerate() {
            let name = literal::string(&source.name);
            output.push_str(&format!("            var owner_{index} = global::Ferrule.Runtime.FerruleXmlInputSource.Named({index}, {name});\n            var input_{index} = extraSources[indices[{static_index}]].Document;\n            global::System.ArgumentNullException.ThrowIfNull(input_{index});\n"));
            let size = if bytes {
                format!("input_{index}.LongLength")
            } else {
                format!(
                    "XmlDynamicDocumentInputAdmissionValue<long>(() => global::Ferrule.Runtime.FerruleXmlInputSetBudget.Measure(owner_{index}, input_{index}))"
                )
            };
            output.push_str(&format!("            var bytes_{index} = {size};\n            XmlDynamicDocumentInputAdmission(() => {{ global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(owner_{index}, bytes_{index}); }});\n"));
        }
        output.push_str("            XmlDynamicDocumentInputAdmission(() => { budget.Charge(primaryOwner, primaryBytes); });\n");
        for (index, _) in &static_sources {
            output.push_str(&format!("            XmlDynamicDocumentInputAdmission(() => {{ budget.Charge(owner_{index}, bytes_{index}); }});\n"));
        }
        output.push_str(&format!("            global::Ferrule.Runtime.FerruleInstance primary;\n            try {{ primary = global::Ferrule.Runtime.FerruleXml.{parser}(SourceXmlSchema, source); }}\n            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)\n            {{ throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentExecutionException.FromInputBoundary(primaryOwner, error); }}\n            var parsedInputs = new global::System.Collections.Generic.List<NamedInput>({});\n", static_sources.len()));
        for (index, source) in &static_sources {
            output.push_str(&format!("            try {{ parsedInputs.Add(new NamedInput({}, global::Ferrule.Runtime.FerruleXml.{parser}(ExtraXmlInputSchema_{index}, input_{index}))); }}\n            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)\n            {{ throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentExecutionException.FromInputBoundary(owner_{index}, error); }}\n", literal::string(&source.name)));
        }
        output.push_str(
            r#"            return (primary, parsedInputs, budget);
    }
"#,
        );
        output.push_str(&format!(r#"
    private static global::System.Collections.Generic.IReadOnlyList<{dto}> {serialize_helper}(ExecutionOutputs mapped)
    {{
        if (mapped.Extras.Count != 0)
            throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentExecutionException.FromDocuments(
                global::Ferrule.Runtime.FerruleXmlDocumentExecutionException.Alignment("dynamic XML documents require no named mapped outputs"));
        if (mapped.Primary is not global::Ferrule.Runtime.FerruleDocumentSet members)
            throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentExecutionException.FromDocuments(
                global::Ferrule.Runtime.FerruleXmlDocumentExecutionException.Alignment("dynamic XML documents require a primary document set"));
        global::Ferrule.Runtime.FerruleXmlDocumentSetBudget budget;
        try {{ budget = new global::Ferrule.Runtime.FerruleXmlDocumentSetBudget(members.Documents.Count); }}
        catch (global::Ferrule.Runtime.FerruleXmlDocumentExecutionException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentExecutionException.FromDocuments(error); }}
        var outputs = new global::System.Collections.Generic.List<{dto}>(members.Documents.Count);
        for (var index = 0; index < members.Documents.Count; index++)
        {{
            var member = members.Documents[index];
            string xml;
            try {{ xml = global::Ferrule.Runtime.FerruleXml.SerializeDocumentEmbedded(TargetXmlSchema, member.Value, {arguments}); }}
            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
            {{
                throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentExecutionException.FromDocuments(
                    global::Ferrule.Runtime.FerruleXmlDocumentExecutionException.Serialization(index, member.Path, error));
            }}
            try {{ budget.Charge(index, member.Path, XmlDynamicDocumentUtf8.GetByteCount(xml)); }}
            catch (global::Ferrule.Runtime.FerruleXmlDocumentExecutionException error)
            {{ throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentExecutionException.FromDocuments(error); }}
            outputs.Add(new {dto}(member.Path, {conversion}));
        }}
        return outputs.AsReadOnly();
    }}
"#, conversion = if bytes { "XmlDynamicDocumentUtf8.GetBytes(xml)" } else { "xml" }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn program() -> Program {
        let project: ::mapping::Project = serde_json::from_str(include_str!("../../../../codegen/src/tests/fixtures/one_dynamic_named_input_dynamic_primary_xml_documents.json")).unwrap();
        codegen::lower(&project).unwrap()
    }
    #[test]
    fn four_distinct_loader_list_methods_own_static_subsequence_and_dynamic_original_index() {
        let mut output = String::new();
        render(&program(), &mut output).unwrap();
        assert_eq!(
            output
                .matches("public static global::System.Collections.Generic.IReadOnlyList<")
                .count(),
            4
        );
        for name in [
            "ExecuteXmlDocumentsWithSourcesAndDynamicSourceLoader(",
            "ExecuteXmlDocumentsWithSourcesContextAndDynamicSourceLoader(",
            "ExecuteXmlBytesDocumentsWithSourcesAndDynamicSourceLoader(",
            "ExecuteXmlBytesDocumentsWithSourcesContextAndDynamicSourceLoader(",
        ] {
            assert_eq!(output.matches(name).count(), 1);
        }
        assert!(output.contains("ExtraXmlInputNames = new string[] { \"rates\", \"labels\" }"));
        assert!(
            output.contains("FerruleXmlDynamicSourcePolicy(1, \"catalog\", ExtraXmlInputSchema_1)")
        );
        assert!(output.contains(
            "owner_2 = global::Ferrule.Runtime.FerruleXmlInputSource.Named(2, \"labels\")"
        ));
        assert!(output.contains("input_2 = extraSources[indices[1]].Document"));
        assert!(output.contains("FromDynamicInputBoundary(request, error.Boundary)"));
        assert!(output.contains("DynamicXmlDocumentInputError(adapter.Recover(error))"));
        assert!(output.contains("outputs.AsReadOnly()"));
        assert!(!output.contains("ExecuteXmlDocuments("));
        assert!(!output.contains("new NamedInput(\"catalog\""));
        assert_eq!(super::super::render_types(&program()).unwrap(), TYPES);
    }
    #[test]
    fn phase_scopes_keep_host_accessors_outside_product_catches_and_count_before_writers() {
        let mut output = String::new();
        render(&program(), &mut output).unwrap();
        assert!(output.contains("var suppliedCount = extraSources.Count;"));
        assert!(output.contains("var input_2 = extraSources[indices[1]].Document;"));
        assert!(output.contains("return (primary, parsedInputs, budget)"));
        let parser = output
            .find("private static (global::Ferrule.Runtime.FerruleInstance Primary,")
            .unwrap();
        let admission = &output[parser..];
        let positions = [
            "var suppliedCount",
            "FerruleXmlInputSetBudget.Indices",
            "foreach (var input in extraSources)",
            "RequireDocumentSize(owner_2",
            "budget.Charge(primaryOwner",
            "ParseStructuredEmbedded(SourceXmlSchema",
            "ParseStructuredEmbedded(ExtraXmlInputSchema_2",
        ]
        .map(|needle| admission.find(needle).unwrap());
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let count = output
            .find(
                "new global::Ferrule.Runtime.FerruleXmlDocumentSetBudget(members.Documents.Count)",
            )
            .unwrap();
        assert!(count < output.find("new global::System.Collections.Generic.List<XmlDocumentOutput>(members.Documents.Count)").unwrap());
        assert!(
            count
                < output
                    .find("SerializeDocumentEmbedded(TargetXmlSchema")
                    .unwrap()
        );
        assert!(
            output.rfind("budget.Charge(index, member.Path").unwrap()
                < output
                    .rfind("XmlDynamicDocumentUtf8.GetBytes(xml)")
                    .unwrap()
        );
        let mut invalid = program();
        invalid.xml_boundary.as_mut().unwrap().extra_inputs[2].name = "wrong".into();
        assert!(super::super::render_types(&invalid).is_err());
        let mut refused = String::new();
        assert!(render(&invalid, &mut refused).is_err());
        assert!(refused.is_empty());
        invalid.xml_boundary = None;
        assert_eq!(super::super::render_types(&invalid).unwrap(), "");
    }
}
