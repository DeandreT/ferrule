//! Isolated physical XML output over the existing selected typed executor.
use crate::{EmitError, literal};
use codegen::{NamedInputAvailability, Program, XmlEmissionPlan};

pub(super) const TYPES: &str = concat!(
    "public sealed record NamedXmlOutput(string Name, string Document);\n",
    "public sealed record NamedXmlBytesOutput(string Name, byte[] Document);\n",
    "public sealed record XmlExecutionOutputs(string Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlOutput> Extras);\n",
    "public sealed record XmlBytesExecutionOutputs(byte[] Primary, global::System.Collections.Generic.IReadOnlyList<NamedXmlBytesOutput> Extras);\n",
    "public sealed record NamedXmlInput(string Name, string Document);\n",
    "public sealed record NamedXmlBytesInput(string Name, byte[] Document);\n",
    "public abstract class SelectedXmlBytesTargetOutput {\n",
    "    private protected SelectedXmlBytesTargetOutput() { }\n",
    "    public sealed class Primary : SelectedXmlBytesTargetOutput { internal Primary(byte[] document) { Document = document; } public byte[] Document { get; } }\n",
    "    public sealed class Named : SelectedXmlBytesTargetOutput { internal Named(NamedXmlBytesOutput output) { Output = output; } public NamedXmlBytesOutput Output { get; } }\n",
    "}\n"
);

pub(super) fn render(
    program: &Program,
    plan: &XmlEmissionPlan<'_>,
    output: &mut String,
) -> Result<(), EmitError> {
    let Some(admission) = plan.selected_inputs() else {
        return Ok(());
    };
    output.push_str(r#"
    public static SelectedXmlBytesTargetOutput ExecuteXmlBytesSelectedTarget(byte[] source,
        global::Ferrule.Runtime.FerruleTargetSelection selection) =>
        ExecuteXmlBytesSelectedTargetWithHost(source, selection, global::System.Array.Empty<NamedXmlBytesInput>());

    public static SelectedXmlBytesTargetOutput ExecuteXmlBytesSelectedTargetWithHost(byte[] source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<NamedXmlBytesInput> extraSources,
        global::Ferrule.Runtime.FerruleExecutionContext? executionContext = null)
    {
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(selection);
        global::System.ArgumentNullException.ThrowIfNull(extraSources);
        int selected;
        try { selected = ResolveTarget(selection); }
        catch (global::Ferrule.Runtime.FerruleRuntimeException error) { throw SelectedXmlMappingError(error); }
        var expected = selected switch
        {
"#);
    for (index, catalogue) in std::iter::once(&admission.primary)
        .chain(
            admission
                .named_targets
                .iter()
                .map(|target| &target.catalogue),
        )
        .enumerate()
    {
        let names = catalogue
            .entries
            .iter()
            .filter(|entry| entry.availability == NamedInputAvailability::RequiredLegacy)
            .map(|entry| literal::string(entry.name))
            .collect::<Vec<_>>()
            .join(", ");
        output.push_str(&format!(
            "            {index} => new string[] {{ {names} }},\n"
        ));
    }
    output.push_str(r#"            _ => throw new global::System.InvalidOperationException(),
        };
        var budget = new global::Ferrule.Runtime.FerruleXmlInputSetBudget((long)extraSources.Count + 1);
        var names = new string[extraSources.Count];
        for (var i = 0; i < names.Length; i++)
        {
            global::System.ArgumentNullException.ThrowIfNull(extraSources[i]);
            global::System.ArgumentNullException.ThrowIfNull(extraSources[i].Name);
            names[i] = extraSources[i].Name;
        }
        var indices = global::Ferrule.Runtime.FerruleXmlInputSetBudget.Indices(expected, names);
        foreach (var input in extraSources) global::System.ArgumentNullException.ThrowIfNull(input.Document);
        var primaryOwner = global::Ferrule.Runtime.FerruleXmlInputSource.Primary;
        global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(primaryOwner, source.LongLength);
        for (var i = 0; i < expected.Length; i++)
            global::Ferrule.Runtime.FerruleXmlInputSetBudget.RequireDocumentSize(SelectedXmlInputOwner(expected[i]), extraSources[indices[i]].Document.LongLength);
        budget.Charge(primaryOwner, source.LongLength);
        for (var i = 0; i < expected.Length; i++)
            budget.Charge(SelectedXmlInputOwner(expected[i]), extraSources[indices[i]].Document.LongLength);
        global::Ferrule.Runtime.FerruleInstance primary;
        try { primary = global::Ferrule.Runtime.FerruleXml.ParseStructuredEmbeddedBytes(SourceXmlSchema, source); }
        catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
        { throw global::Ferrule.Runtime.FerruleXmlExecutionException.ForInput(primaryOwner, error); }
        var fields = new global::Ferrule.Runtime.FerruleField[expected.Length];
        for (var i = 0; i < expected.Length; i++)
        {
            var name = expected[i];
            try { fields[i] = new(name, global::Ferrule.Runtime.FerruleXml.ParseStructuredEmbeddedBytes(SelectedXmlInputSchema(name), extraSources[indices[i]].Document)); }
            catch (global::Ferrule.Runtime.FerruleXmlBoundaryException error)
            { throw global::Ferrule.Runtime.FerruleXmlExecutionException.ForInput(SelectedXmlInputOwner(name), error); }
        }
        var context = global::Ferrule.Runtime.ScopeContext.FromSources(primary, fields, executionContext);
        SelectedTargetOutput mapped;
        try { mapped = ExecuteSelectedTargetFromContext(context, selected); }
        catch (global::Ferrule.Runtime.FerruleRuntimeException error) { throw SelectedXmlMappingError(error); }
        try
        {
            var outputBudget = new global::Ferrule.Runtime.FerruleXmlOutputSetBudget(1);
            switch (selected)
            {
"#);
    for (index, (schema, policy, name)) in
        std::iter::once(("TargetXmlSchema".to_string(), &plan.policy().output, None))
            .chain(program.extra_targets.iter().enumerate().map(|(i, target)| {
                (
                    format!("ExtraXmlSchema_{i}"),
                    &plan.policy().extra_outputs[i].output,
                    Some(target.name.as_str()),
                )
            }))
            .enumerate()
    {
        let args = super::output_arguments(policy)?;
        let (pattern, instance, owner, result) = match name {
            None => (
                "SelectedTargetOutput.Primary value".to_string(),
                "value.Instance",
                "global::Ferrule.Runtime.FerruleXmlOutputTarget.Primary".to_string(),
                "new SelectedXmlBytesTargetOutput.Primary(bytes)".to_string(),
            ),
            Some(name) => {
                let name = literal::string(name);
                (
                    format!("SelectedTargetOutput.Named value && value.Output.Name == {name}"),
                    "value.Output.Instance",
                    format!(
                        "global::Ferrule.Runtime.FerruleXmlOutputTarget.Named({}, {name})",
                        index - 1
                    ),
                    format!(
                        "new SelectedXmlBytesTargetOutput.Named(new NamedXmlBytesOutput({name}, bytes))"
                    ),
                )
            }
        };
        output.push_str(&format!("                case {index} when mapped is {pattern}:\n                {{\n                    var owner = {owner};\n                    var xml = SerializeXmlTarget(owner, {schema}, {instance}, {args});\n                    outputBudget.Charge(owner, XmlOutputUtf8.GetByteCount(xml));\n                    var bytes = XmlOutputUtf8.GetBytes(xml);\n                    return {result};\n                }}\n"));
    }
    output.push_str(r#"                default: throw global::Ferrule.Runtime.FerruleXmlOutputSetBudget.Alignment("XML selected output does not match its target");
            }
        }
        catch (global::Ferrule.Runtime.FerruleXmlOutputSetException error)
        { throw global::Ferrule.Runtime.FerruleXmlExecutionException.FromOutput(error); }
    }
    private static global::Ferrule.Runtime.FerruleXmlExecutionException SelectedXmlMappingError(global::Ferrule.Runtime.FerruleRuntimeException error) =>
        global::Ferrule.Runtime.FerruleXmlExecutionException.Unowned(new global::Ferrule.Runtime.FerruleXmlBoundaryException(global::Ferrule.Runtime.FerruleXmlBoundaryErrorKind.Mapping, error.Message, error));
    private static global::Ferrule.Runtime.FerruleXmlInputSource SelectedXmlInputOwner(string name) => name switch
    {
"#);
    for (i, source) in program.extra_sources.iter().enumerate() {
        let name = literal::string(&source.name);
        output.push_str(&format!(
            "        {name} => global::Ferrule.Runtime.FerruleXmlInputSource.Named({i}, {name}),\n"
        ));
    }
    output.push_str("        _ => throw new global::System.InvalidOperationException(),\n    };\n    private static string SelectedXmlInputSchema(string name) => name switch\n    {\n");
    for (i, source) in program.extra_sources.iter().enumerate() {
        output.push_str(&format!(
            "        {} => ExtraXmlInputSchema_{i},\n",
            literal::string(&source.name)
        ));
    }
    output.push_str("        _ => throw new global::System.InvalidOperationException(),\n    };\n");
    Ok(())
}
