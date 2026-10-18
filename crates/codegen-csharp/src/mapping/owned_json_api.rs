//! Strict document companions for the explicit, control-free Basic entry family.
//! Admission owns the only snapshot/parsed buffers; generated code never revisits
//! the caller list or rebuilds a typed NamedInput descriptor collection.

use codegen::Program;
use ir::SchemaNode;

use crate::{EmitError, literal};

pub(super) fn render(program: &Program, output: &mut String) -> Result<(), EmitError> {
    output.push_str(JSON_TYPES);
    output.push_str(super::selected_json_api::OUTPUT_TYPES);
    output.push_str("public static partial class GeneratedMapping\n{\n");
    render_schema("BasicOwnedSourceJsonSchema", &program.source, output)?;
    render_schema("BasicOwnedTargetJsonSchema", &program.target, output)?;
    // Original declaration ordinals, including reserved-unavailable entries,
    // are the sole key shared by the immutable catalogue and parser callback.
    for (index, source) in program.extra_sources.iter().enumerate() {
        render_schema(
            &format!("BasicOwnedSourceJsonSchema_{index}"),
            &source.source,
            output,
        )?;
    }
    for (index, target) in program.extra_targets.iter().enumerate() {
        render_schema(
            &format!("BasicOwnedTargetJsonSchema_{index}"),
            &target.target,
            output,
        )?;
    }
    output.push_str(INPUT_ACCESSORS);
    render_named_parser(program, output);
    for bytes in [false, true] {
        render_all_entry(bytes, output);
        render_selected_entry(program, bytes, output);
        render_all_serializer(program, bytes, output);
        render_selected_serializer(program, bytes, output);
    }
    output.push_str("}\n");
    Ok(())
}

fn render_schema(name: &str, schema: &SchemaNode, output: &mut String) -> Result<(), EmitError> {
    let descriptor =
        codegen::serialize_embedded_schema(schema, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)?;
    output.push_str(&format!(
        "    private const string {name} = {};\n",
        literal::string(&descriptor)
    ));
    Ok(())
}

const JSON_TYPES: &str = r#"
public sealed record NamedJsonInput(
    string Name,
    string Document);

public sealed record NamedJsonBytesInput(
    string Name,
    byte[] Document);

public sealed record NamedJsonOutput(
    string Name,
    string Document);

public sealed record NamedJsonBytesOutput(
    string Name,
    byte[] Document);

public sealed record JsonExecutionOutputs(
    string Primary,
    global::System.Collections.Generic.IReadOnlyList<NamedJsonOutput> Extras);

public sealed record JsonBytesExecutionOutputs(
    byte[] Primary,
    global::System.Collections.Generic.IReadOnlyList<NamedJsonBytesOutput> Extras);

"#;

const INPUT_ACCESSORS: &str = r#"
    // Closed handles capture references only. Admission reads Count once and
    // each reached item once; neither parsing nor binding re-reads this list.
    private sealed class BasicOwnedJsonInputAccessor :
        global::Ferrule.Runtime.IFerruleNamedJsonDocumentAccessor<NamedJsonInput>
    {
        internal static readonly BasicOwnedJsonInputAccessor Instance = new();
        private BasicOwnedJsonInputAccessor() { }
        public global::Ferrule.Runtime.FerruleNamedJsonDocumentRef Read(NamedJsonInput? item)
            => item is null ? default : new(item.Name,
                global::Ferrule.Runtime.FerruleJsonDocumentRef.FromText(item.Document));
    }

    private sealed class BasicOwnedJsonBytesInputAccessor :
        global::Ferrule.Runtime.IFerruleNamedJsonDocumentAccessor<NamedJsonBytesInput>
    {
        internal static readonly BasicOwnedJsonBytesInputAccessor Instance = new();
        private BasicOwnedJsonBytesInputAccessor() { }
        public global::Ferrule.Runtime.FerruleNamedJsonDocumentRef Read(NamedJsonBytesInput? item)
            => item is null ? default : new(item.Name,
                global::Ferrule.Runtime.FerruleJsonDocumentRef.FromUtf8(item.Document));
    }
"#;

fn render_named_parser(program: &Program, output: &mut String) {
    output.push_str(
        r#"
    private static global::Ferrule.Runtime.FerruleInstance ParseBasicOwnedNamedJsonDocument(
        int declarationIndex, global::Ferrule.Runtime.FerruleJsonDocumentRef document)
    {
"#,
    );
    if program.extra_sources.is_empty() {
        output.push_str("        throw new global::System.InvalidOperationException(\"Admitted JSON declaration is invalid.\");\n    }\n");
        return;
    }
    output.push_str(
        r#"        string schema = declarationIndex switch
        {
"#,
    );
    for index in 0..program.extra_sources.len() {
        output.push_str(&format!(
            "            {index} => BasicOwnedSourceJsonSchema_{index},\n"
        ));
    }
    output.push_str(
        r#"            _ => throw new global::System.InvalidOperationException("Admitted JSON declaration is invalid."),
        };
        return document.Kind switch
        {
            global::Ferrule.Runtime.FerruleJsonDocumentKind.Text =>
                global::Ferrule.Runtime.FerruleJson.ParseEmbedded(schema, document.Text!),
            global::Ferrule.Runtime.FerruleJsonDocumentKind.Utf8 =>
                global::Ferrule.Runtime.FerruleJson.ParseEmbeddedBytes(schema, document.Utf8!),
            _ => throw new global::System.InvalidOperationException("Admitted JSON document kind is invalid."),
        };
    }
"#,
    );
}

fn render_all_entry(bytes: bool, output: &mut String) {
    let (stem, document, input, parse, accessor) = if bytes {
        (
            "JsonBytes",
            "byte[]",
            "NamedJsonBytesInput",
            "ParseEmbeddedBytes",
            "BasicOwnedJsonBytesInputAccessor",
        )
    } else {
        (
            "Json",
            "string",
            "NamedJsonInput",
            "ParseEmbedded",
            "BasicOwnedJsonInputAccessor",
        )
    };
    output.push_str(&format!(
        r#"
    public static {stem}ExecutionOutputs Execute{stem}OutputsWithBasicOwnedSources(
        {document} source,
        global::System.Collections.Generic.IReadOnlyList<{input}> inputs,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(inputs);
        var admission = global::Ferrule.Runtime.FerruleBasicJsonInputAdmission.Admit(
            CheckedAllInputs, inputs, {accessor}.Instance, limits);
        var primary = global::Ferrule.Runtime.FerruleJson.{parse}(BasicOwnedSourceJsonSchema, source);
        var parsed = admission.ParseNamedWith(ParseBasicOwnedNamedJsonDocument);
        try
        {{
            var roots = parsed.BasicRoots(primary);
            var context = roots.RootContext(execution);
            try
            {{
                var outcome = (ExecutionOutputs)ExecuteCheckedBasicContext(context, null);
                return SerializeBasicOwned{stem}Outputs(outcome);
            }}
            finally {{ global::System.GC.KeepAlive(roots); }}
        }}
        finally {{ global::System.GC.KeepAlive(parsed); }}
    }}
"#
    ));
}

fn render_selected_entry(program: &Program, bytes: bool, output: &mut String) {
    let (stem, document, input, parse, accessor) = if bytes {
        (
            "JsonBytes",
            "byte[]",
            "NamedJsonBytesInput",
            "ParseEmbeddedBytes",
            "BasicOwnedJsonBytesInputAccessor",
        )
    } else {
        (
            "Json",
            "string",
            "NamedJsonInput",
            "ParseEmbedded",
            "BasicOwnedJsonInputAccessor",
        )
    };
    output.push_str(&format!(
        r#"
    public static Selected{stem}TargetOutput Execute{stem}SelectedTargetWithBasicOwnedSources(
        {document} source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<{input}> inputs,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(selection);
        var selected = ResolveOwnedBasicTarget(selection);
        global::System.ArgumentNullException.ThrowIfNull(inputs);
        var catalogue = selected switch
        {{
            0 => CheckedPrimaryInputs,
"#
    ));
    for index in 0..program.extra_targets.len() {
        output.push_str(&format!(
            "            {} => CheckedNamedInputs_{index},\n",
            index + 1
        ));
    }
    output.push_str(&format!(
        r#"            _ => throw new global::System.InvalidOperationException("Resolved checked target is invalid."),
        }};
        var admission = global::Ferrule.Runtime.FerruleBasicJsonInputAdmission.Admit(
            catalogue, inputs, {accessor}.Instance, limits);
        var primary = global::Ferrule.Runtime.FerruleJson.{parse}(BasicOwnedSourceJsonSchema, source);
        var parsed = admission.ParseNamedWith(ParseBasicOwnedNamedJsonDocument);
        try
        {{
            var roots = parsed.BasicRoots(primary);
            var context = roots.RootContext(execution);
            try
            {{
                var outcome = (SelectedTargetOutput)ExecuteCheckedBasicContext(context, selected);
                return SerializeBasicOwnedSelected{stem}Target(outcome, selected);
            }}
            finally {{ global::System.GC.KeepAlive(roots); }}
        }}
        finally {{ global::System.GC.KeepAlive(parsed); }}
    }}
"#
    ));
}

fn render_all_serializer(program: &Program, bytes: bool, output: &mut String) {
    let (stem, serialize, named) = if bytes {
        (
            "JsonBytes",
            "SerializeEmbeddedCompactBytes",
            "NamedJsonBytesOutput",
        )
    } else {
        ("Json", "SerializeEmbeddedCompact", "NamedJsonOutput")
    };
    output.push_str(&format!(
        r#"
    private static {stem}ExecutionOutputs SerializeBasicOwned{stem}Outputs(ExecutionOutputs outcome)
    {{
        var primary = global::Ferrule.Runtime.FerruleJson.{serialize}(BasicOwnedTargetJsonSchema, outcome.Primary);
"#
    ));
    for index in 0..program.extra_targets.len() {
        output.push_str(&format!(
            "        var extra_{index} = global::Ferrule.Runtime.FerruleJson.{serialize}(BasicOwnedTargetJsonSchema_{index}, outcome.Extras[{index}].Instance);\n"
        ));
    }
    output.push_str(&format!(
        "        return new {stem}ExecutionOutputs(primary, new {named}[]\n        {{\n"
    ));
    for (index, target) in program.extra_targets.iter().enumerate() {
        output.push_str(&format!(
            "            new({}, extra_{index}),\n",
            literal::string(&target.name)
        ));
    }
    output.push_str("        });\n    }\n");
}

fn render_selected_serializer(program: &Program, bytes: bool, output: &mut String) {
    let (stem, serialize, named) = if bytes {
        (
            "JsonBytes",
            "SerializeEmbeddedCompactBytes",
            "NamedJsonBytesOutput",
        )
    } else {
        ("Json", "SerializeEmbeddedCompact", "NamedJsonOutput")
    };
    output.push_str(&format!(
        r#"
    private static Selected{stem}TargetOutput SerializeBasicOwnedSelected{stem}Target(
        SelectedTargetOutput outcome, int selected)
    {{
        switch (selected)
        {{
            case 0 when outcome is SelectedTargetOutput.Primary primary:
                return new Selected{stem}TargetOutput.Primary(
                    global::Ferrule.Runtime.FerruleJson.{serialize}(BasicOwnedTargetJsonSchema, primary.Instance));
"#
    ));
    for (index, target) in program.extra_targets.iter().enumerate() {
        let selected = index + 1;
        let name = literal::string(&target.name);
        output.push_str(&format!(
            r#"            case {selected} when outcome is SelectedTargetOutput.Named named_{selected} &&
                global::System.String.Equals(named_{selected}.Output.Name, {name}, global::System.StringComparison.Ordinal):
                return new Selected{stem}TargetOutput.Named(new {named}(
                    named_{selected}.Output.Name,
                    global::Ferrule.Runtime.FerruleJson.{serialize}(BasicOwnedTargetJsonSchema_{index}, named_{selected}.Output.Instance)));
"#
        ));
    }
    output.push_str(
        r#"            default:
                throw new global::Ferrule.Runtime.FerruleRuntimeException(
                    global::Ferrule.Runtime.FerruleRuntimeError.JsonBoundary,
                    "generated mapping returned a target that does not match the resolved selection");
        }
    }
"#,
    );
}
