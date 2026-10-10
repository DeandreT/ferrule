use codegen::Program;

use crate::literal;

pub(super) const OUTPUT_TYPES: &str = r#"public abstract class SelectedJsonTargetOutput
{
    private protected SelectedJsonTargetOutput() { }

    public sealed class Primary : SelectedJsonTargetOutput
    {
        internal Primary(string document) { Document = document; }
        public string Document { get; }
    }

    public sealed class Named : SelectedJsonTargetOutput
    {
        internal Named(NamedJsonOutput output) { Output = output; }
        public NamedJsonOutput Output { get; }
    }
}

public abstract class SelectedJsonBytesTargetOutput
{
    private protected SelectedJsonBytesTargetOutput() { }

    public sealed class Primary : SelectedJsonBytesTargetOutput
    {
        internal Primary(byte[] document) { Document = document; }
        public byte[] Document { get; }
    }

    public sealed class Named : SelectedJsonBytesTargetOutput
    {
        internal Named(NamedJsonBytesOutput output) { Output = output; }
        public NamedJsonBytesOutput Output { get; }
    }
}

"#;

pub(super) fn render(program: &Program, output: &mut String) {
    let dynamic = program
        .extra_sources
        .iter()
        .any(|source| source.dynamic.is_some());
    for bytes in [false, true] {
        let (method, result, document, input, parse, names, named_parse, serialize, named_output) =
            if bytes {
                (
                    "ExecuteJsonBytesSelectedTarget",
                    "SelectedJsonBytesTargetOutput",
                    "byte[]",
                    "NamedJsonBytesInput",
                    "ParseEmbeddedBytes",
                    "ValidateNamedJsonBytesInputNames",
                    "ParseNamedJsonBytesInputs",
                    "SerializeEmbeddedBytes",
                    "NamedJsonBytesOutput",
                )
            } else {
                (
                    "ExecuteJsonSelectedTarget",
                    "SelectedJsonTargetOutput",
                    "string",
                    "NamedJsonInput",
                    "ParseEmbedded",
                    "ValidateNamedJsonInputNames",
                    "ParseNamedJsonInputs",
                    "SerializeEmbedded",
                    "NamedJsonOutput",
                )
            };
        output.push_str(&format!(
            r#"
    public static {result} {method}(
        {document} source,
        global::Ferrule.Runtime.FerruleTargetSelection selection)
    {{
        return {method}WithHost(source, selection, global::System.Array.Empty<{input}>());
    }}

    public static {result} {method}WithHost(
        {document} source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<{input}> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext = null,
        global::Ferrule.Runtime.IFerruleDynamicJsonSourceLoader? loader = null)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(selection);
        var selected = ResolveTarget(selection);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        {names}(extraSources);
        var parsedSource = global::Ferrule.Runtime.FerruleJson.{parse}(SourceJsonSchema, source);
        var parsedInputs = {named_parse}(extraSources);
"#
        ));
        if dynamic {
            output.push_str("        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? adapter = loader is null\n            ? null\n            : new GeneratedDynamicJsonSourceLoader(loader);\n");
        } else {
            output.push_str(
                "        global::Ferrule.Runtime.IFerruleDynamicSourceLoader? adapter = null;\n",
            );
        }
        output.push_str(&format!(r#"        var outcome = ExecuteSelectedTargetWithHost(
            parsedSource, selection, parsedInputs, executionContext, adapter);
        return Serialize{result}(outcome, selected);
    }}

    private static {result} Serialize{result}(SelectedTargetOutput outcome, int selected)
    {{
        switch (selected)
        {{
            case 0 when outcome is SelectedTargetOutput.Primary primary:
                return new {result}.Primary(
                    global::Ferrule.Runtime.FerruleJson.{serialize}(TargetJsonSchema, primary.Instance));
"#));
        for (index, target) in program.extra_targets.iter().enumerate() {
            let selected = index + 1;
            let name = literal::string(&target.name);
            output.push_str(&format!(r#"            case {selected} when outcome is SelectedTargetOutput.Named named_{selected} &&
                global::System.String.Equals(named_{selected}.Output.Name, {name}, global::System.StringComparison.Ordinal):
                return new {result}.Named(new {named_output}(
                    named_{selected}.Output.Name,
                    global::Ferrule.Runtime.FerruleJson.{serialize}(ExtraTargetJsonSchemas[{index}], named_{selected}.Output.Instance)));
"#));
        }
        output.push_str(r#"            default:
                throw new global::Ferrule.Runtime.FerruleRuntimeException(
                    global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,
                    "generated mapping returned a target that does not match the resolved selection");
        }
    }
"#);
    }
}
