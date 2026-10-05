//! Static Structured named inputs beside an ordinary or observed primary.
//! Complete input preflight precedes either primary reader.

use crate::{EmitError, literal};
use codegen::Program;

pub(super) const TYPES: &str = "public sealed record NamedXmlOutput(string Name, string Document);\npublic sealed record NamedXmlBytesOutput(string Name, byte[] Document);\npublic sealed record XmlExecutionOutputs(string Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlOutput> Extras);\npublic sealed record XmlBytesExecutionOutputs(byte[] Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlBytesOutput> Extras);\n\npublic sealed record NamedXmlInput(string Name, string Document);\npublic sealed record NamedXmlBytesInput(string Name, byte[] Document);\n";

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
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
    let static_sources = program
        .extra_sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.dynamic.is_none())
        .collect::<Vec<_>>();
    let has_dynamic = program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_some());
    let budget_return_type = if has_dynamic {
        ", global::Ferrule.Runtime.FerruleXmlInputSetBudget Budget"
    } else {
        ""
    };
    let budget_return_value = if has_dynamic { ", budget" } else { "" };
    for bytes in [false, true] {
        let stem = if bytes {
            "ExecuteXmlBytes"
        } else {
            "ExecuteXml"
        };
        let input = if bytes {
            "NamedXmlBytesInput"
        } else {
            "NamedXmlInput"
        };
        let ty = if bytes { "byte[]" } else { "string" };
        let result = if bytes {
            "XmlBytesExecutionOutputs"
        } else {
            "XmlExecutionOutputs"
        };
        let parser = if bytes {
            "ParseStructuredEmbeddedBytes"
        } else {
            "ParseStructuredEmbedded"
        };
        let observed_primary = program.xml_boundary.as_ref().is_some_and(|policy| {
            policy.input.profile() == Some(codegen::XmlInputProfile::RootView)
        });
        let primary_parser = if observed_primary {
            if bytes {
                "ParseEmbeddedBytes"
            } else {
                "ParseEmbedded"
            }
        } else {
            parser
        };
        let primary_arguments = if observed_primary { ", true, true" } else { "" };
        let parse_helper = if bytes {
            "ParseXmlBytesInputs"
        } else {
            "ParseXmlInputs"
        };
        let serialize = if bytes {
            "SerializeXmlBytesOutputs"
        } else {
            "SerializeXmlOutputs"
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
    public static {result} {stem}OutputsWithSources({ty} source,
        global::System.Collections.Generic.IReadOnlyList<{input}> extraSources{context_arg})
    {{
{require_context}        var parsed = {parse_helper}(source, extraSources);
        ExecutionOutputs mapped;
        try {{ mapped = ExecuteOutputsWithSources(parsed.Primary, parsed.Inputs{context_call}); }}
        catch (global::Ferrule.Runtime.FerruleRuntimeException error)
        {{
            throw global::Ferrule.Runtime.FerruleXmlExecutionException.Unowned(
                new global::Ferrule.Runtime.FerruleXmlBoundaryException(
                    global::Ferrule.Runtime.FerruleXmlBoundaryErrorKind.Mapping, error.Message, error));
        }}
        try {{ return {serialize}(mapped); }}
        catch (global::Ferrule.Runtime.FerruleXmlOutputSetException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlExecutionException.FromOutput(error); }}
    }}

    public static {ty} {stem}WithSources({ty} source,
        global::System.Collections.Generic.IReadOnlyList<{input}> extraSources{context_arg}) =>
        {stem}OutputsWithSources(source, extraSources{context_call}).Primary;

    public static {result} {stem}Outputs({ty} source{context_arg})
    {{
        try {{ return {stem}OutputsWithSources(source, global::System.Array.Empty<{input}>(){context_call}); }}
        catch (global::Ferrule.Runtime.FerruleXmlExecutionException error) {{ throw error.ToOutputSet(); }}
    }}

    public static {ty} {stem}({ty} source{context_arg})
    {{
        try {{ return {stem}Outputs(source{context_call}).Primary; }}
        catch (global::Ferrule.Runtime.FerruleXmlOutputSetException error) {{ throw error.Boundary; }}
    }}
"#));
        }
        output.push_str(&format!(r#"
    private static (global::Ferrule.Runtime.FerruleInstance Primary,
        global::System.Collections.Generic.IReadOnlyList<NamedInput> Inputs{budget_return_type}) {parse_helper}(
        {ty} source, global::System.Collections.Generic.IReadOnlyList<{input}> extraSources)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        var budget = new global::Ferrule.Runtime.FerruleXmlInputSetBudget((long)extraSources.Count + 1);
        var suppliedNames = new string[extraSources.Count];
        for (var i = 0; i < extraSources.Count; i++)
        {{
            global::System.ArgumentNullException.ThrowIfNull(extraSources[i]);
            global::System.ArgumentNullException.ThrowIfNull(extraSources[i].Name);
            suppliedNames[i] = extraSources[i].Name;
        }}
        var indices = global::Ferrule.Runtime.FerruleXmlInputSetBudget.Indices(ExtraXmlInputNames, suppliedNames);
        _ = indices;
        // Complete exact names precede complete Document shape; neither phase decodes XML.
        foreach (var input in extraSources)
            global::System.ArgumentNullException.ThrowIfNull(input.Document);
        var primaryOwner = global::Ferrule.Runtime.FerruleXmlInputSource.Primary;
"#));
        let primary_size = if bytes {
            "source.LongLength".to_owned()
        } else {
            "global::Ferrule.Runtime.FerruleXmlInputSetBudget.Measure(primaryOwner, source)"
                .to_owned()
        };
        output.push_str(&format!("        var primaryBytes = {primary_size};\n        global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(primaryOwner, primaryBytes);\n"));
        for (static_index, (index, source)) in static_sources.iter().enumerate() {
            let name = literal::string(&source.name);
            output.push_str(&format!("        var owner_{index} = global::Ferrule.Runtime.FerruleXmlInputSource.Named({index}, {name});\n        var input_{index} = extraSources[indices[{static_index}]].Document;\n        global::System.ArgumentNullException.ThrowIfNull(input_{index});\n"));
            let size = if bytes {
                format!("input_{index}.LongLength")
            } else {
                format!(
                    "global::Ferrule.Runtime.FerruleXmlInputSetBudget.Measure(owner_{index}, input_{index})"
                )
            };
            output.push_str(&format!("        var bytes_{index} = {size};\n        global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(owner_{index}, bytes_{index});\n"));
        }
        output.push_str("        budget.Charge(primaryOwner, primaryBytes);\n");
        for (index, _) in &static_sources {
            output.push_str(&format!(
                "        budget.Charge(owner_{index}, bytes_{index});\n"
            ));
        }
        output.push_str(&format!("        global::Ferrule.Runtime.FerruleInstance primary;\n        try {{ primary = global::Ferrule.Runtime.FerruleXml.{primary_parser}(SourceXmlSchema, source{primary_arguments}); }}\n        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)\n        {{ throw global::Ferrule.Runtime.FerruleXmlExecutionException.ForInput(primaryOwner, error); }}\n        var parsedInputs = new global::System.Collections.Generic.List<NamedInput>({});\n", static_sources.len()));
        for (index, source) in &static_sources {
            output.push_str(&format!("        try {{ parsedInputs.Add(new NamedInput({}, global::Ferrule.Runtime.FerruleXml.{parser}(ExtraXmlInputSchema_{index}, input_{index}))); }}\n        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)\n        {{ throw global::Ferrule.Runtime.FerruleXmlExecutionException.ForInput(owner_{index}, error); }}\n", literal::string(&source.name)));
        }
        output.push_str(&format!(
            "        return (primary, parsedInputs{budget_return_value});\n    }}\n"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    fn project() -> ::mapping::Project {
        serde_json::from_str(include_str!(
            "../../../../codegen/src/tests/fixtures/root_view_static_named_xml_inputs.json"
        ))
        .unwrap()
    }
    #[test]
    fn observed_primary_and_structured_named_parsers_keep_whole_input_phase_order() {
        let program = codegen::lower(&project()).unwrap();
        let mut output = String::new();
        super::super::render(&program, &mut output).unwrap();
        assert!(
            super::super::render_types(&program)
                .unwrap()
                .contains("public sealed record NamedXmlInput")
        );
        assert_eq!(
            output
                .matches("ParseEmbedded(SourceXmlSchema, source, true, true)")
                .count(),
            1
        );
        assert_eq!(
            output
                .matches("ParseEmbeddedBytes(SourceXmlSchema, source, true, true)")
                .count(),
            1
        );
        assert!(output.contains("ParseStructuredEmbedded(ExtraXmlInputSchema_0"));
        assert!(output.contains("ParseStructuredEmbeddedBytes(ExtraXmlInputSchema_1"));
        let parse_start = output
            .find("private static (global::Ferrule.Runtime.FerruleInstance Primary,")
            .unwrap();
        let parser = &output[parse_start..];
        let positions = [
            "new global::Ferrule.Runtime.FerruleXmlInputSetBudget",
            "FerruleXmlInputSetBudget.Indices",
            "foreach (var input in extraSources)",
            "RequireDocumentSize(owner_1",
            "budget.Charge(primaryOwner",
            "budget.Charge(owner_1",
            "ParseEmbedded(SourceXmlSchema",
            "ParseStructuredEmbedded(ExtraXmlInputSchema_0",
            "ParseStructuredEmbedded(ExtraXmlInputSchema_1",
        ]
        .map(|needle| parser.find(needle).unwrap());
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(output.matches("    public static ").count(), 16);
        assert!(output.contains("FerruleXmlExecutionException.ForInput(owner_1, error)"));
        assert!(output.contains("FerruleXmlExecutionException.Unowned("));
        let mut invalid = program;
        invalid.xml_boundary.as_mut().unwrap().extra_inputs[1].name = "wrong".into();
        let mut refused = String::new();
        assert!(super::super::render(&invalid, &mut refused).is_err());
        assert!(refused.is_empty());
    }
    #[test]
    fn zero_named_observed_route_keeps_its_existing_types_and_boundary_wrappers() {
        let project: ::mapping::Project = serde_json::from_str(include_str!(
            "../../../../codegen/src/tests/fixtures/static_named_xml_rootview.json"
        ))
        .unwrap();
        let program = codegen::lower(&project).unwrap();
        assert!(
            !super::super::render_types(&program)
                .unwrap()
                .contains("NamedXmlInput")
        );
        let mut output = String::new();
        super::super::render(&program, &mut output).unwrap();
        assert!(!output.contains("WithSources"));
        assert!(output.contains("ParseEmbedded(SourceXmlSchema, source, true, true)"));
        assert!(output.contains("throw error.Boundary"));
        assert_eq!(output.matches("    public static ").count(), 8);
    }
}
