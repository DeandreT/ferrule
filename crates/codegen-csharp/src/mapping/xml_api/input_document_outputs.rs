//! Closed static input admission plus unchanged primary-first mixed serializer.
use super::output_arguments;
use crate::{EmitError, literal};
use codegen::{Program, ProgramValidationError};

pub(super) const TYPES: &str = "public sealed record NamedXmlInput(string Name, string Document);\npublic sealed record NamedXmlBytesInput(string Name, byte[] Document);\npublic sealed record XmlDocumentOutput(string Path, string Document);\npublic sealed record XmlBytesDocumentOutput(string Path, byte[] Document);\npublic sealed record NamedXmlDocumentOutputs(string Name, global::System.Collections.Generic.IReadOnlyList<XmlDocumentOutput> Documents);\npublic sealed record NamedXmlBytesDocumentOutputs(string Name, global::System.Collections.Generic.IReadOnlyList<XmlBytesDocumentOutput> Documents);\npublic sealed record XmlDocumentExecutionOutputs(string Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlDocumentOutputs> Extras);\npublic sealed record XmlBytesDocumentExecutionOutputs(byte[] Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlBytesDocumentOutputs> Extras);\n\n";

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
    if program.xml_output_mode()?
        != Some(codegen::XmlOutputMode::StaticNamedInputsStaticPrimaryDynamicNamedDocuments)
    {
        return Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "static input mixed XML outputs require their checked adapter mode".into(),
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
    // Only sealed static-admission calls enter these conversions. Host list
    // accessors and ordinary CLR shape guards remain outside this channel.
    private static T XmlDocumentOutputsInputAdmissionValue<T>(global::System.Func<T> action)
    {
        try { return action(); }
        catch (global::Ferrule.Runtime.FerruleXmlExecutionException error)
        { throw global::Ferrule.Runtime.FerruleXmlInputDocumentOutputsExecutionException.FromInputBoundary(error.Input, error.Boundary); }
    }
    private static void XmlDocumentOutputsInputAdmission(global::System.Action action)
    {
        try { action(); }
        catch (global::Ferrule.Runtime.FerruleXmlExecutionException error)
        { throw global::Ferrule.Runtime.FerruleXmlInputDocumentOutputsExecutionException.FromInputBoundary(error.Input, error.Boundary); }
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
    output.push_str("    private static readonly global::System.Text.UTF8Encoding XmlDocumentOutputsUtf8 = new(false, true);\n");
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
            "ParseXmlBytesDocumentInputs"
        } else {
            "ParseXmlDocumentInputs"
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
        {ty} source, global::System.Collections.Generic.IReadOnlyList<{input}> extraSources{context_arg})
    {{
{require_context}        var parsed = {parse_helper}(source, extraSources);
        ExecutionOutputs mapped;
        try {{ mapped = ExecuteOutputsWithSources(parsed.Primary, parsed.Inputs{context_call}); }}
        catch (global::Ferrule.Runtime.FerruleRuntimeException error)
        {{
            throw global::Ferrule.Runtime.FerruleXmlInputDocumentOutputsExecutionException.FromBoundary(
                new global::Ferrule.Runtime.FerruleXmlBoundaryException(
                    global::Ferrule.Runtime.FerruleXmlBoundaryErrorKind.Mapping, error.Message, error));
        }}
        try {{ return {helper}(mapped); }}
        catch (global::Ferrule.Runtime.FerruleXmlDocumentOutputsExecutionException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlInputDocumentOutputsExecutionException.FromOutputs(error); }}
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
            var budget = XmlDocumentOutputsInputAdmissionValue<global::Ferrule.Runtime.FerruleXmlInputSetBudget>(
                () => new global::Ferrule.Runtime.FerruleXmlInputSetBudget((long)suppliedCount + 1));
            var suppliedNames = new string[suppliedCount];
            for (var i = 0; i < suppliedCount; i++)
            {{
                global::System.ArgumentNullException.ThrowIfNull(extraSources[i]);
                global::System.ArgumentNullException.ThrowIfNull(extraSources[i].Name);
                suppliedNames[i] = extraSources[i].Name;
            }}
            var indices = XmlDocumentOutputsInputAdmissionValue<int[]>(() =>
                global::Ferrule.Runtime.FerruleXmlInputSetBudget.Indices(ExtraXmlInputNames, suppliedNames));
            // Complete names precede complete Document shape, then all sizes.
            foreach (var input in extraSources)
                global::System.ArgumentNullException.ThrowIfNull(input.Document);
            var primaryOwner = global::Ferrule.Runtime.FerruleXmlInputSource.Primary;
"#));
        let primary_size = if bytes {
            "source.LongLength"
        } else {
            "XmlDocumentOutputsInputAdmissionValue<long>(() => global::Ferrule.Runtime.FerruleXmlInputSetBudget.Measure(primaryOwner, source))"
        };
        output.push_str(&format!("            var primaryBytes = {primary_size};\n            XmlDocumentOutputsInputAdmission(() => {{ global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(primaryOwner, primaryBytes); }});\n"));
        for (index, source) in program.extra_sources.iter().enumerate() {
            let name = literal::string(&source.name);
            output.push_str(&format!("            var owner_{index} = global::Ferrule.Runtime.FerruleXmlInputSource.Named({index}, {name});\n            var input_{index} = extraSources[indices[{index}]].Document;\n            global::System.ArgumentNullException.ThrowIfNull(input_{index});\n"));
            let size = if bytes {
                format!("input_{index}.LongLength")
            } else {
                format!(
                    "XmlDocumentOutputsInputAdmissionValue<long>(() => global::Ferrule.Runtime.FerruleXmlInputSetBudget.Measure(owner_{index}, input_{index}))"
                )
            };
            output.push_str(&format!("            var bytes_{index} = {size};\n            XmlDocumentOutputsInputAdmission(() => {{ global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(owner_{index}, bytes_{index}); }});\n"));
        }
        output.push_str("            XmlDocumentOutputsInputAdmission(() => { budget.Charge(primaryOwner, primaryBytes); });\n");
        for index in 0..program.extra_sources.len() {
            output.push_str(&format!("            XmlDocumentOutputsInputAdmission(() => {{ budget.Charge(owner_{index}, bytes_{index}); }});\n"));
        }
        output.push_str(&format!("            global::Ferrule.Runtime.FerruleInstance primary;\n            try {{ primary = global::Ferrule.Runtime.FerruleXml.{parser}(SourceXmlSchema, source); }}\n            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)\n            {{ throw global::Ferrule.Runtime.FerruleXmlInputDocumentOutputsExecutionException.FromInputBoundary(primaryOwner, error); }}\n            var parsedInputs = new global::System.Collections.Generic.List<NamedInput>({});\n", program.extra_sources.len()));
        for (index, source) in program.extra_sources.iter().enumerate() {
            output.push_str(&format!("            try {{ parsedInputs.Add(new NamedInput({}, global::Ferrule.Runtime.FerruleXml.{parser}(ExtraXmlInputSchema_{index}, input_{index}))); }}\n            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)\n            {{ throw global::Ferrule.Runtime.FerruleXmlInputDocumentOutputsExecutionException.FromInputBoundary(owner_{index}, error); }}\n", literal::string(&source.name)));
        }
        output.push_str(
            r#"            return (primary, parsedInputs);
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
    fn project() -> ::mapping::Project {
        serde_json::from_str(include_str!("../../../../codegen/src/tests/fixtures/static_named_inputs_static_primary_dynamic_named_xml_documents.json")).unwrap()
    }
    fn program() -> Program {
        codegen::lower(&project()).unwrap()
    }

    #[test]
    fn exports_four_sources_overloads_with_existing_readonly_mixed_dtos() {
        let program = program();
        assert_eq!(super::super::render_types(&program).unwrap(), TYPES);
        let mut output = String::new();
        super::super::render(&program, &mut output).unwrap();
        let functions = output
            .lines()
            .filter(|line| line.trim_start().starts_with("public static "))
            .collect::<Vec<_>>();
        assert_eq!(functions.len(), 4);
        assert_eq!(
            functions
                .iter()
                .filter(|line| line.contains(" ExecuteXmlDocumentOutputsWithSources("))
                .count(),
            2
        );
        assert_eq!(
            functions
                .iter()
                .filter(|line| line.contains(" ExecuteXmlBytesDocumentOutputsWithSources("))
                .count(),
            2
        );
        assert!(TYPES.contains("NamedXmlInput(string Name, string Document)"));
        assert!(TYPES.contains("NamedXmlBytesInput(string Name, byte[] Document)"));
        assert!(TYPES.contains("XmlDocumentExecutionOutputs(string Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlDocumentOutputs> Extras)"));
        assert!(
            output.contains("FerruleXmlInputDocumentOutputsExecutionException.FromOutputs(error)")
        );
        assert!(!output.contains(" ExecuteXml("));
        assert!(!output.contains(" ExecuteXmlDocumentsWithSources("));
        assert!(!output.contains(" ExecuteXmlDocumentOutputs("));
        assert!(!output.contains("FerruleXmlDynamicSourceAdapter"));
        assert!(output.contains("documents0.AsReadOnly()"));
        assert!(output.contains("documents1.AsReadOnly()"));
        assert!(output.contains("extras.AsReadOnly()"));
    }

    #[test]
    fn all_input_and_target_declarations_keep_own_schemas_policies_and_original_indices() {
        let mut project = project();
        for reversed in [false, true] {
            if reversed {
                project.extra_targets.swap(0, 1);
            }
            let program = codegen::lower(&project).unwrap();
            let mut output = String::new();
            render(&program, &mut output).unwrap();
            for (index, input) in program.extra_sources.iter().enumerate() {
                let descriptor = codegen::serialize_embedded_schema(
                    &input.source,
                    codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
                )
                .unwrap();
                assert!(output.contains(&format!(
                    "private const string ExtraXmlInputSchema_{index} = {};",
                    literal::string(&descriptor)
                )));
                assert!(output.contains(&format!(
                    "FerruleXmlInputSource.Named({index}, {})",
                    literal::string(&input.name)
                )));
                assert!(output.contains(&format!(
                    "ParseStructuredEmbedded(ExtraXmlInputSchema_{index}, input_{index})"
                )));
                assert!(output.contains(&format!(
                    "ParseStructuredEmbeddedBytes(ExtraXmlInputSchema_{index}, input_{index})"
                )));
            }
            assert_eq!(program.extra_sources.len(), 3);
            for (index, target) in program.extra_targets.iter().enumerate() {
                let descriptor = codegen::serialize_embedded_schema(
                    &target.target,
                    codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
                )
                .unwrap();
                assert!(output.contains(&format!(
                    "private const string NamedXmlDocumentSchema{index} = {};",
                    literal::string(&descriptor)
                )));
                assert!(output.contains(&format!(
                    "private const string NamedXmlDocumentOutputName{index} = {};",
                    literal::string(&target.name)
                )));
                let policy = &program.xml_boundary.as_ref().unwrap().extra_outputs[index].output;
                assert!(output.contains(&format!(
                    "SerializeDocumentEmbedded(NamedXmlDocumentSchema{index}, member.Value, {})",
                    output_arguments(policy).unwrap()
                )));
                assert!(output.contains(&format!("NamedSerialization({index}, NamedXmlDocumentOutputName{index}, index, member.Path, error)")));
            }
        }
    }

    #[test]
    fn shapes_all_sizes_original_parsing_then_mapping_and_checked_output_count_remain_ordered() {
        let mut output = String::new();
        render(&program(), &mut output).unwrap();
        let count = output
            .find("FerruleXmlInputSetBudget((long)suppliedCount + 1)")
            .unwrap();
        let names = output
            .find("FerruleXmlInputSetBudget.Indices(ExtraXmlInputNames")
            .unwrap();
        let shape = output.find("ThrowIfNull(input.Document)").unwrap();
        let last_size = output
            .find("RequireDocumentSize(owner_2, bytes_2)")
            .unwrap();
        let charge = output
            .find("budget.Charge(primaryOwner, primaryBytes)")
            .unwrap();
        let primary = output
            .find("ParseStructuredEmbedded(SourceXmlSchema, source)")
            .unwrap();
        let last_input = output
            .find("ParseStructuredEmbedded(ExtraXmlInputSchema_2, input_2)")
            .unwrap();
        assert!(
            count < names
                && names < shape
                && shape < last_size
                && last_size < charge
                && charge < primary
                && primary < last_input
        );
        // Parse helper completes before the one typed call in each public method.
        assert!(output.contains("var parsed = ParseXmlDocumentInputs(source, extraSources);\n        ExecutionOutputs mapped;\n        try { mapped = ExecuteOutputsWithSources(parsed.Primary, parsed.Inputs); }"));
        let alignment = output.find("FerruleDocumentSet members1").unwrap();
        let first_count = output
            .find("artifactCount = checked(artifactCount + members0.Documents.Count)")
            .unwrap();
        let last_count = output
            .find("artifactCount = checked(artifactCount + members1.Documents.Count)")
            .unwrap();
        let budget = output
            .find("FerruleXmlDocumentOutputsBudget(artifactCount)")
            .unwrap();
        let serialize = output
            .find("SerializeDocumentEmbedded(TargetXmlSchema")
            .unwrap();
        let primary_charge = output.find("budget.ChargePrimary(").unwrap();
        let named = output
            .find("SerializeDocumentEmbedded(NamedXmlDocumentSchema0")
            .unwrap();
        assert!(
            alignment < first_count
                && first_count < last_count
                && last_count < budget
                && budget < serialize
                && serialize < primary_charge
                && primary_charge < named
        );
        assert!(
            output.find("budget.ChargeNamed(1,").unwrap() < output.find("documents1.Add(").unwrap()
        );
        assert!(output.contains("var primary = XmlDocumentOutputsUtf8.GetBytes(primaryXml)"));
        assert!(output.contains("member.Path, XmlDocumentOutputsUtf8.GetBytes(xml)"));
        assert!(output.contains("var suppliedCount = extraSources.Count;"));
    }

    #[test]
    fn invalid_later_policy_refuses_before_types_or_methods_and_old_dispatch_is_exact() {
        for input in [true, false] {
            let mut invalid = program();
            if input {
                invalid.xml_boundary.as_mut().unwrap().extra_inputs[2].name = "wrong".into();
            } else {
                invalid.xml_boundary.as_mut().unwrap().extra_outputs[1].name = "wrong".into();
            }
            let mut output = String::new();
            assert!(matches!(
                super::super::render(&invalid, &mut output),
                Err(EmitError::ProgramValidation(_))
            ));
            assert!(output.is_empty());
            assert!(matches!(
                super::super::render_types(&invalid),
                Err(EmitError::ProgramValidation(_))
            ));
        }
        for (fixture, expected) in [
            (
                include_str!(
                    "../../../../codegen/src/tests/fixtures/static_named_inputs_dynamic_primary_xml_documents.json"
                ),
                " ExecuteXmlDocumentsWithSources(",
            ),
            (
                include_str!(
                    "../../../../codegen/src/tests/fixtures/static_primary_multiple_dynamic_named_xml_documents.json"
                ),
                " ExecuteXmlDocumentOutputs(",
            ),
        ] {
            let project: ::mapping::Project = serde_json::from_str(fixture).unwrap();
            let program = codegen::lower(&project).unwrap();
            let mut output = String::new();
            super::super::render(&program, &mut output).unwrap();
            assert!(output.contains(expected));
            assert!(!output.contains("ExecuteXmlDocumentOutputsWithSources"));
            assert!(!output.contains("FerruleXmlInputDocumentOutputsExecutionException"));
            assert_ne!(super::super::render_types(&program).unwrap(), TYPES);
        }
    }
}
