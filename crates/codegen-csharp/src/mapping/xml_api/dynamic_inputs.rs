//! Additive raw-byte dynamic loader methods for the Structured XML profile.

use crate::literal;
use codegen::Program;

pub(super) fn render(program: &Program, output: &mut String) {
    let dynamic_sources = program
        .extra_sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.dynamic.is_some())
        .collect::<Vec<_>>();
    let Some((dynamic_index, dynamic)) = dynamic_sources.first().copied() else {
        return;
    };
    let adapter = if dynamic_sources.len() == 1 {
        format!(
            "        var adapter = new global::Ferrule.Runtime.FerruleXmlDynamicSourceAdapter(loader,\n            new global::Ferrule.Runtime.FerruleXmlDynamicSourcePolicy({dynamic_index}, {}, ExtraXmlInputSchema_{dynamic_index}), parsed.Budget);\n",
            literal::string(&dynamic.name)
        )
    } else {
        let mut adapter = String::from(
            "        var adapter = global::Ferrule.Runtime.FerruleXmlDynamicSourceAdapter.ForSources(loader,\n            new global::Ferrule.Runtime.FerruleXmlDynamicSourcePolicy[] {\n",
        );
        for (index, source) in dynamic_sources {
            adapter.push_str(&format!("                new global::Ferrule.Runtime.FerruleXmlDynamicSourcePolicy({index}, {}, ExtraXmlInputSchema_{index}),\n", literal::string(&source.name)));
        }
        adapter.push_str("            }, parsed.Budget);\n");
        adapter
    };
    for bytes in [false, true] {
        let (stem, input, ty, result, parse_helper, serialize) = if bytes {
            (
                "ExecuteXmlBytes",
                "NamedXmlBytesInput",
                "byte[]",
                "XmlBytesExecutionOutputs",
                "ParseXmlBytesInputs",
                "SerializeXmlBytesOutputs",
            )
        } else {
            (
                "ExecuteXml",
                "NamedXmlInput",
                "string",
                "XmlExecutionOutputs",
                "ParseXmlInputs",
                "SerializeXmlOutputs",
            )
        };
        for context in [false, true] {
            let (suffix, context_arg, context_call, require_context, execute) = if context {
                (
                    "WithSourcesContextAndDynamicSourceLoader",
                    ", global::Ferrule.Runtime.FerruleExecutionContext executionContext",
                    ", executionContext",
                    "        global::System.ArgumentNullException.ThrowIfNull(executionContext);\n",
                    "ExecuteOutputsWithSourcesContextAndDynamicSourceLoader",
                )
            } else {
                (
                    "WithSourcesAndDynamicSourceLoader",
                    "",
                    "",
                    "",
                    "ExecuteOutputsWithSourcesAndDynamicSourceLoader",
                )
            };
            let name = format!("{stem}Outputs{suffix}");
            let singular = format!("{stem}{suffix}");
            output.push_str(&format!(r#"
    public static {result} {name}({ty} source,
        global::System.Collections.Generic.IReadOnlyList<{input}> extraSources{context_arg},
        global::Ferrule.Runtime.IFerruleDynamicXmlSourceLoader loader)
    {{
{require_context}        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        global::System.ArgumentNullException.ThrowIfNull(loader);
        var parsed = {parse_helper}(source, extraSources);
{adapter}        ExecutionOutputs mapped;
        try {{ mapped = {execute}(parsed.Primary, parsed.Inputs{context_call}, adapter); }}
        catch (global::Ferrule.Runtime.FerruleRuntimeException error) {{ throw adapter.Recover(error); }}
        try {{ return {serialize}(mapped); }}
        catch (global::Ferrule.Runtime.FerruleXmlOutputSetException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlExecutionException.FromOutput(error); }}
    }}

    public static {ty} {singular}({ty} source,
        global::System.Collections.Generic.IReadOnlyList<{input}> extraSources{context_arg},
        global::Ferrule.Runtime.IFerruleDynamicXmlSourceLoader loader) =>
        {name}(source, extraSources{context_call}, loader).Primary;
"#));
        }
    }
}
