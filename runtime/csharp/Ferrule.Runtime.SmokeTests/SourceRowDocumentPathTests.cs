using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void SourceRowDocumentPaths()
    {
        static FerruleGroup Mark(FerruleGroup row, string path, string? resolved = null) =>
            row.WithSourceOrigin(new FerruleSourceDocumentOrigin(path, resolved).Row(1));
        static FerruleGroup Empty() => new(Array.Empty<FerruleField>());
        static void Error(ScopeContext context, string[] path, FerruleSourceRowPathError expected)
        {
            try { context.ResolveSourceRowDocumentPath(27, path); }
            catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.SourceRowDocumentPath && error.Node == 27 &&
                error.InnerException is FerruleSourceRowPathException cause && cause.Error == expected && cause.Collection.SequenceEqual(path))
            { return; }
            throw new InvalidOperationException("Row path did not preserve its exact typed failure.");
        }
        var input = new FerruleGroup(new FerruleField[] { new("Rows", new FerruleRepeated(new FerruleInstance[] {
            Mark(Empty(), "same.csv", " physical/A.csv "), Mark(Empty(), "same.csv", "physical/B.csv"),
        })) });
        var root = ScopeContext.FromSource(input);
        Error(root, ["Rows"], FerruleSourceRowPathError.InactiveFrame);
        var rows = root.IterateSource("Rows");
        if (rows[0].ResolveSourceRowDocumentPath(27, ["Rows"]).StringValue != " physical/A.csv " ||
            rows[1].ResolveSourceRowDocumentPath(27, ["Rows"]).StringValue != "physical/B.csv")
            throw new InvalidOperationException("Row paths were normalized or borrowed.");
        Error(rows[0], ["rows"], FerruleSourceRowPathError.InactiveFrame);
        var grouped = root.GroupBy(rows, ["Rows"], _ => FerruleValue.FromInt64(1));
        Error(grouped[0], ["Rows"], FerruleSourceRowPathError.GroupedFrame);
        var members = grouped[0].IterateSource(Array.Empty<string>());
        if (members[0].ResolveSourceRowDocumentPath(27, ["Rows"]).StringValue != " physical/A.csv " ||
            members[1].ResolveSourceRowDocumentPath(27, ["Rows"]).StringValue != "physical/B.csv")
            throw new InvalidOperationException("Grouping lost physical row provenance.");
        var nested = new FerruleRepeated(new FerruleInstance[] { Mark(new FerruleGroup(new FerruleField[] {
            new("Children", new FerruleRepeated(new FerruleInstance[] { Empty(), Mark(Empty(), "child.csv") }))
        }), "outer.csv") });
        var outer = ScopeContext.FromSource(nested).IterateSource(Array.Empty<string>())[0];
        var children = outer.IterateSource("Children");
        Error(children[0], ["Children"], FerruleSourceRowPathError.MissingOrigin);
        Error(children[1], ["x", "Children"], FerruleSourceRowPathError.InactiveFrame);
        if (children[0].ResolveSourceRowDocumentPath(27, []).StringValue != "outer.csv" ||
            children[1].ResolveSourceRowDocumentPath(27, ["Children"]).StringValue != "child.csv")
            throw new InvalidOperationException("Nested row ownership changed.");
        Error(ScopeContext.FromSource(Mark(Empty(), "root.csv")), [], FerruleSourceRowPathError.InactiveFrame);
        var malformed = new FerruleRepeated(new FerruleInstance[] { new FerruleRepeated(new FerruleInstance[] { Empty() }) });
        Error(ScopeContext.FromSource(malformed).IterateSource(Array.Empty<string>())[0], [], FerruleSourceRowPathError.GroupedFrame);
    }
}
