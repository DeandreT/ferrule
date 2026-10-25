//! Owned XML document-set entry points. Every endpoint arrives as an ordered
//! member list; names are admitted like the typed entry points before any
//! member is read, and the parsed sets run through the same checked factory.
use crate::{EmitError, literal};
use codegen::Program;

pub(crate) fn render(program: &Program) -> Result<String, EmitError> {
    let schema = |node| {
        codegen::serialize_embedded_schema(node, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)
            .map(|schema| literal::string(&schema))
            .map_err(EmitError::from)
    };
    let extras = program
        .extra_sources
        .iter()
        .map(|source| {
            Ok(format!(
                "({}, {})",
                literal::string(&source.name),
                schema(&source.source)?
            ))
        })
        .collect::<Result<Vec<_>, EmitError>>()?;
    let mut catalogues = String::from("            0 => CheckedPrimaryInputs,\n");
    for index in 0..program.extra_targets.len() {
        catalogues.push_str(&format!(
            "            {} => CheckedNamedInputs_{index},\n",
            index + 1
        ));
    }
    let mut output = format!(
        r#"
// Owned XML document-set entry points: each endpoint is an ordered member list
// read through the structured XML profile, keeping every member's path and
// resolved location.
public static partial class GeneratedMapping
{{
    private const string BasicOwnedSourceXmlSchema = {};
    private static readonly (string Name, string Schema)[] BasicOwnedExtraSourceXmlSchemas = new (string, string)[] {{ {} }};
    // Name admission never inspects a source tree, so one empty set stands in.
    private static readonly global::Ferrule.Runtime.FerruleInstance BasicOwnedXmlDocumentSetPlaceholder =
        new global::Ferrule.Runtime.FerruleDocumentSet(global::System.Array.Empty<global::Ferrule.Runtime.FerruleDocument>());

    // Names first, then every member's shape and size, then each set in order:
    // the primary, then the supplied named sets.
    private static (global::Ferrule.Runtime.FerruleInstance Primary, NamedInput[] Inputs) ParseBasicOwnedXmlDocumentSets<T>(
        global::Ferrule.Runtime.FerruleBasicNamedSourceDeclaration[] catalogue,
        global::System.Collections.Generic.IReadOnlyList<T> source,
        (string Name, global::System.Collections.Generic.IReadOnlyList<T> Members)[] supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::System.Action<global::System.Collections.Generic.IReadOnlyList<(global::Ferrule.Runtime.FerruleXmlInputSource Source, global::System.Collections.Generic.IReadOnlyList<T> Members)>> preflight,
        global::System.Func<string, global::Ferrule.Runtime.FerruleXmlInputSource, global::System.Collections.Generic.IReadOnlyList<T>, global::Ferrule.Runtime.FerruleInstance> parse)
    {{
        var placeholders = new NamedInput[supplied.Length];
        for (var i = 0; i < supplied.Length; i++)
            placeholders[i] = new NamedInput(supplied[i].Name, BasicOwnedXmlDocumentSetPlaceholder);
        try
        {{
            _ = global::Ferrule.Runtime.FerruleBasicSourceRoots.CreateFromNamedInputs(
                BasicOwnedXmlDocumentSetPlaceholder, catalogue, placeholders, new CheckedNamedInputAccessor(placeholders), limits);
        }}
        catch (global::Ferrule.Runtime.FerruleRuntimeException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlDocumentSetInputException.FromMapping(error); }}
        var sets = new global::System.Collections.Generic.List<(global::Ferrule.Runtime.FerruleXmlInputSource Source, global::System.Collections.Generic.IReadOnlyList<T> Members)>(supplied.Length + 1)
        {{
            (global::Ferrule.Runtime.FerruleXmlInputSource.Primary, source),
        }};
        var schemas = new string[supplied.Length];
        for (var i = 0; i < supplied.Length; i++)
        {{
            var index = global::System.Array.FindIndex(BasicOwnedExtraSourceXmlSchemas,
                declared => global::System.String.Equals(declared.Name, supplied[i].Name, global::System.StringComparison.Ordinal));
            if (index < 0)
                throw global::Ferrule.Runtime.FerruleXmlDocumentSetInputException.FromMapping(new global::Ferrule.Runtime.FerruleRuntimeException(
                    global::Ferrule.Runtime.FerruleRuntimeError.UnexpectedNamedSource,
                    $"named source '{{supplied[i].Name}}' is not declared by this mapping", detail: supplied[i].Name));
            var (name, schema) = BasicOwnedExtraSourceXmlSchemas[index];
            sets.Add((global::Ferrule.Runtime.FerruleXmlInputSource.Named(index, name), supplied[i].Members));
            schemas[i] = schema;
        }}
        preflight(sets);
        var primary = parse(BasicOwnedSourceXmlSchema, global::Ferrule.Runtime.FerruleXmlInputSource.Primary, source);
        var inputs = new NamedInput[supplied.Length];
        for (var i = 0; i < supplied.Length; i++)
            inputs[i] = new NamedInput(supplied[i].Name, parse(schemas[i], sets[i + 1].Source, sets[i + 1].Members));
        return (primary, inputs);
    }}

    private static global::Ferrule.Runtime.FerruleBasicNamedSourceDeclaration[] BasicOwnedXmlDocumentSetCatalogue(
        global::Ferrule.Runtime.FerruleTargetSelection selection)
    {{
        int selected;
        try {{ selected = ResolveOwnedBasicTarget(selection); }}
        catch (global::Ferrule.Runtime.FerruleRuntimeException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlDocumentSetInputException.FromMapping(error); }}
        return selected switch
        {{
{catalogues}            _ => throw new global::System.InvalidOperationException("Resolved checked target is invalid."),
        }};
    }}
"#,
        schema(&program.source)?,
        extras.join(", "),
    );
    for bytes in [false, true] {
        let (infix, member, named) = if bytes {
            (
                "XmlBytes",
                "FerruleXmlBytesDocumentSetMember",
                "FerruleNamedXmlBytesDocumentSet",
            )
        } else {
            (
                "Xml",
                "FerruleXmlDocumentSetMember",
                "FerruleNamedXmlDocumentSet",
            )
        };
        output.push_str(&format!(
            r#"
    private static (string Name, global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.{member}> Members)[] BasicOwned{infix}DocumentSets(
        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.{named}> supplied)
    {{
        global::System.ArgumentNullException.ThrowIfNull(supplied);
        var sets = new (string Name, global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.{member}> Members)[supplied.Count];
        for (var i = 0; i < sets.Length; i++)
        {{
            var set = supplied[i];
            global::System.ArgumentNullException.ThrowIfNull(set);
            global::System.ArgumentNullException.ThrowIfNull(set.Name);
            sets[i] = (set.Name, set.Members);
        }}
        return sets;
    }}

    public static ExecutionOutputs Execute{infix}DocumentSetsWithBasicOwnedSources(
        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.{member}> source,
        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.{named}> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        var (primary, inputs) = ParseBasicOwnedXmlDocumentSets(CheckedAllInputs, source, BasicOwned{infix}DocumentSets(supplied), limits,
            global::Ferrule.Runtime.FerruleXmlDocumentSets.Preflight, global::Ferrule.Runtime.FerruleXmlDocumentSets.Parse);
        try {{ return ExecuteCheckedOwnedBasic(primary, inputs, limits, execution); }}
        catch (global::Ferrule.Runtime.FerruleRuntimeException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlDocumentSetInputException.FromMapping(error); }}
    }}

    public static SelectedTargetOutput ExecuteSelected{infix}DocumentSetsWithBasicOwnedSources(
        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.{member}> source,
        global::Ferrule.Runtime.FerruleTargetSelection selection,
        global::System.Collections.Generic.IReadOnlyList<global::Ferrule.Runtime.{named}> supplied,
        global::Ferrule.Runtime.FerruleOwnedSourceLimits limits,
        global::Ferrule.Runtime.FerruleExecutionContext? execution)
    {{
        global::System.ArgumentNullException.ThrowIfNull(source);
        global::System.ArgumentNullException.ThrowIfNull(selection);
        var catalogue = BasicOwnedXmlDocumentSetCatalogue(selection);
        var (primary, inputs) = ParseBasicOwnedXmlDocumentSets(catalogue, source, BasicOwned{infix}DocumentSets(supplied), limits,
            global::Ferrule.Runtime.FerruleXmlDocumentSets.Preflight, global::Ferrule.Runtime.FerruleXmlDocumentSets.Parse);
        try {{ return ExecuteSelectedCheckedOwnedBasic(primary, selection, inputs, limits, execution); }}
        catch (global::Ferrule.Runtime.FerruleRuntimeException error)
        {{ throw global::Ferrule.Runtime.FerruleXmlDocumentSetInputException.FromMapping(error); }}
    }}
"#
        ));
    }
    output.push_str("}\n");
    Ok(output)
}
