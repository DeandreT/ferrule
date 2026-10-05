//! One dynamic XML input and optional statics feed primary-first named document lists.
use super::output_arguments;
use crate::{EmitError, literal};
use codegen::{Program, ProgramValidationError};

pub(super) const TYPES: &str = super::input_document_outputs::TYPES;

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
    if program.xml_output_mode()?
        != Some(codegen::XmlOutputMode::DynamicNamedInputStaticPrimaryDynamicNamedDocuments)
    {
        return Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "dynamic input mixed XML outputs require their checked adapter mode".into(),
        }
        .into());
    }
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
            reason: "dynamic input mixed XML outputs require their dynamic declaration".into(),
        })?;
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
    output.push_str(r#"
    // Only trusted static-admission operations enter these helpers. Host list
    // accessors and ordinary CLR shape guards remain outside their catches.
    private static T XmlDynamicDocumentOutputsStaticAdmissionValue<T>(global::System.Func<T> action)
    {
        try { return action(); }
        catch (global::Ferrule.Runtime.FerruleXmlExecutionException error)
        {
            global::System.Diagnostics.Debug.Assert(error.Output is null && error.Request is null);
            throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentOutputsExecutionException.FromInputBoundary(error.Input, error.Boundary);
        }
    }
    private static void XmlDynamicDocumentOutputsStaticAdmission(global::System.Action action)
    {
        try { action(); }
        catch (global::Ferrule.Runtime.FerruleXmlExecutionException error)
        {
            global::System.Diagnostics.Debug.Assert(error.Output is null && error.Request is null);
            throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentOutputsExecutionException.FromInputBoundary(error.Input, error.Boundary);
        }
    }
"#);
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
    output.push_str("    private static readonly global::System.Text.UTF8Encoding XmlDynamicDocumentOutputsUtf8 = new(false, true);\n");
    let primary_arguments = output_arguments(&policy.output)?;
    for bytes in [false, true] {
        let name = if bytes {
            "ExecuteXmlBytesDocumentOutputsWithSources"
        } else {
            "ExecuteXmlDocumentOutputsWithSources"
        };
        let input = if bytes {
            "NamedXmlBytesInput"
        } else {
            "NamedXmlInput"
        };
        let ty = if bytes { "byte[]" } else { "string" };
        let parse_helper = if bytes {
            "ParseXmlBytesDynamicDocumentOutputsInputs"
        } else {
            "ParseXmlDynamicDocumentOutputsInputs"
        };
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
            "SerializeXmlBytesDynamicInputDocumentOutputs"
        } else {
            "SerializeXmlDynamicInputDocumentOutputs"
        };
        let conversion = if bytes {
            "XmlDynamicDocumentOutputsUtf8.GetBytes(xml)"
        } else {
            "xml"
        };
        let primary_conversion = if bytes {
            "XmlDynamicDocumentOutputsUtf8.GetBytes(primaryXml)"
        } else {
            "primaryXml"
        };
        for context in [false, true] {
            let name = format!(
                "{name}{}",
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
    public static {result} {name}(
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
        {{
            // Only this fresh terminal adapter's original synchronous recovery
            // can supply an authenticated Input/Request to the generated channel.
            var original = adapter.Recover(error);
            if (original.Request is {{ }} request)
            {{
                global::System.Diagnostics.Debug.Assert(original.Output is null);
                global::System.Diagnostics.Debug.Assert(original.Input == global::Ferrule.Runtime.FerruleXmlInputSource.Named(request.DeclarationIndex, request.Source));
                throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentOutputsExecutionException.FromDynamicInputBoundary(request, original.Boundary);
            }}
            global::System.Diagnostics.Debug.Assert(original.Input is null && original.Output is null);
            throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentOutputsExecutionException.FromBoundary(original.Boundary);
        }}
        try {{ return {helper}(mapped); }}
        catch (global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentOutputsExecutionException.FromOutputs(error); }}
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
            var budget = XmlDynamicDocumentOutputsStaticAdmissionValue<global::Ferrule.Runtime.FerruleXmlInputSetBudget>(
                () => new global::Ferrule.Runtime.FerruleXmlInputSetBudget((long)suppliedCount + 1));
            var suppliedNames = new string[suppliedCount];
            for (var i = 0; i < suppliedCount; i++)
            {{
                global::System.ArgumentNullException.ThrowIfNull(extraSources[i]);
                global::System.ArgumentNullException.ThrowIfNull(extraSources[i].Name);
                suppliedNames[i] = extraSources[i].Name;
            }}
            var indices = XmlDynamicDocumentOutputsStaticAdmissionValue<int[]>(() =>
                global::Ferrule.Runtime.FerruleXmlInputSetBudget.Indices(ExtraXmlInputNames, suppliedNames));
            // Complete names precede complete Document shape, then all sizes.
            foreach (var input in extraSources)
                global::System.ArgumentNullException.ThrowIfNull(input.Document);
            var primaryOwner = global::Ferrule.Runtime.FerruleXmlInputSource.Primary;
"#));
        let primary_size = if bytes {
            "source.LongLength"
        } else {
            "XmlDynamicDocumentOutputsStaticAdmissionValue<long>(() => global::Ferrule.Runtime.FerruleXmlInputSetBudget.Measure(primaryOwner, source))"
        };
        output.push_str(&format!("            var primaryBytes = {primary_size};\n            XmlDynamicDocumentOutputsStaticAdmission(() => {{ global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(primaryOwner, primaryBytes); }});\n"));
        for (static_index, (index, source)) in static_sources.iter().enumerate() {
            let name = literal::string(&source.name);
            output.push_str(&format!("            var owner_{index} = global::Ferrule.Runtime.FerruleXmlInputSource.Named({index}, {name});\n            var input_{index} = extraSources[indices[{static_index}]].Document;\n            global::System.ArgumentNullException.ThrowIfNull(input_{index});\n"));
            let size = if bytes {
                format!("input_{index}.LongLength")
            } else {
                format!(
                    "XmlDynamicDocumentOutputsStaticAdmissionValue<long>(() => global::Ferrule.Runtime.FerruleXmlInputSetBudget.Measure(owner_{index}, input_{index}))"
                )
            };
            output.push_str(&format!("            var bytes_{index} = {size};\n            XmlDynamicDocumentOutputsStaticAdmission(() => {{ global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(owner_{index}, bytes_{index}); }});\n"));
        }
        output.push_str("            XmlDynamicDocumentOutputsStaticAdmission(() => { budget.Charge(primaryOwner, primaryBytes); });\n");
        for (index, _) in &static_sources {
            output.push_str(&format!("            XmlDynamicDocumentOutputsStaticAdmission(() => {{ budget.Charge(owner_{index}, bytes_{index}); }});\n"));
        }
        output.push_str(&format!("            global::Ferrule.Runtime.FerruleInstance primary;\n            try {{ primary = global::Ferrule.Runtime.FerruleXml.{parser}(SourceXmlSchema, source); }}\n            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)\n            {{ throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentOutputsExecutionException.FromInputBoundary(primaryOwner, error); }}\n            var parsedInputs = new global::System.Collections.Generic.List<NamedInput>({});\n", static_sources.len()));
        for (index, source) in &static_sources {
            output.push_str(&format!("            try {{ parsedInputs.Add(new NamedInput({}, global::Ferrule.Runtime.FerruleXml.{parser}(ExtraXmlInputSchema_{index}, input_{index}))); }}\n            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)\n            {{ throw global::Ferrule.Runtime.FerruleXmlDynamicInputDocumentOutputsExecutionException.FromInputBoundary(owner_{index}, error); }}\n", literal::string(&source.name)));
        }
        output.push_str(
            r#"            return (primary, parsedInputs, budget);
    }
"#,
        );
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
        budget.ChargePrimary(XmlDynamicDocumentOutputsUtf8.GetByteCount(primaryXml));
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
            budget.ChargeNamed({declaration_index}, NamedXmlDocumentOutputName{declaration_index}, index, member.Path, XmlDynamicDocumentOutputsUtf8.GetByteCount(xml));
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
