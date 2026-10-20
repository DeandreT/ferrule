using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleGroup HierarchyInput(params FerruleValue[] paths) =>
        Group(Field("Files", Group(Field("File",
            new FerruleRepeated(paths.Select(path => (FerruleInstance)Scalar(path)))))));

    private static string HierarchyName(FerruleInstance instance) =>
        ((FerruleScalar)((FerruleGroup)instance).Fields.Single(field => field.Name == "name").Value)
            .Value.StringValue;

    private static IReadOnlyList<FerruleInstance> HierarchyItems(FerruleInstance instance, string field) =>
        ((FerruleRepeated)((FerruleGroup)instance).Fields.Single(candidate => candidate.Name == field).Value)
            .Items;

    private static FerruleInstance OwnedHierarchy(FerruleBasicScopeContext context,
        FerruleOwnedSourceOwner owner) =>
        FerrulePathHierarchy.BuildOwned(context, owner, new[] { "Files", "File" }, "/",
            "directory", "file", "name");

    private static void PathHierarchyLegacyBuild()
    {
        var tree = FerrulePathHierarchy.Build(ScopeContext.FromSource(HierarchyInput(
                Text("r/b/x"), Text("r/a"), FerruleValue.Null, Text("top"), Text("r/b/x"))),
            new[] { "Files", "File" }, "/", "directory", "file", "name");
        Equal("r", HierarchyName(tree));
        Equal("file,directory,name", string.Join(",", ((FerruleGroup)tree).Fields.Select(field => field.Name)));
        Equal("a", HierarchyName(HierarchyItems(tree, "file").Single()));
        var nested = HierarchyItems(tree, "directory").Single();
        Equal("b", HierarchyName(nested));
        Equal(2, HierarchyItems(nested, "file").Count);
        var roots = Error(FerruleRuntimeError.PathHierarchyRootCount, () => FerrulePathHierarchy.Build(
            ScopeContext.FromSource(HierarchyInput(Text("one/a"), Text("two/b"))),
            new[] { "Files", "File" }, "/", "directory", "file", "name"));
        Equal("2", roots.Detail);
        var type = Error(FerruleRuntimeError.PathHierarchyValueType, () => FerrulePathHierarchy.Build(
            ScopeContext.FromSource(HierarchyInput(Text("r/a"), FerruleValue.FromInt64(1))),
            new[] { "Files", "File" }, "/", "directory", "file", "name"));
        Equal("Int64", type.FoundInstance);
    }

    private static void PathHierarchyOwnedEndpoints()
    {
        var primary = HierarchyInput(Text("p/a"), Text("p/sub/b"));
        var audit = HierarchyInput(Text("n/x"), FerruleValue.Null, Text("top"));
        // A named endpoint spelled like a primary field never supplies the primary collection.
        var alias = HierarchyInput(Text("alias/q"));
        var declared = new[]
        {
            new FerruleDeclaredNamedSourceRoot("audit", audit),
            new FerruleDeclaredNamedSourceRoot("Files", alias),
            new FerruleDeclaredNamedSourceRoot("missing", null),
        };
        var context = FerruleBasicSourceRoots.Create(primary, declared, BasicLimits).RootContext();
        var tree = OwnedHierarchy(context, FerruleOwnedSourceOwner.Primary);
        Equal("p", HierarchyName(tree));
        Equal("a", HierarchyName(HierarchyItems(tree, "file").Single()));
        Equal("sub", HierarchyName(HierarchyItems(tree, "directory").Single()));
        var named = OwnedHierarchy(context, FerruleOwnedSourceOwner.Named("audit"));
        Equal("n", HierarchyName(named));
        Equal("x", HierarchyName(HierarchyItems(named, "file").Single()));
        Equal(0, HierarchyItems(named, "directory").Count);
        Equal("alias", HierarchyName(OwnedHierarchy(context, FerruleOwnedSourceOwner.Named("Files"))));

        var unavailable = Error(FerruleRuntimeError.OwnedSource,
            () => OwnedHierarchy(context, FerruleOwnedSourceOwner.Named("missing")));
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, unavailable.OwnedSourceFailure!.Error);

        var empty = FerruleBasicSourceRoots.Create(HierarchyInput(), Array.Empty<FerruleDeclaredNamedSourceRoot>(),
            BasicLimits).RootContext();
        Equal("0", Error(FerruleRuntimeError.PathHierarchyRootCount,
            () => OwnedHierarchy(empty, FerruleOwnedSourceOwner.Primary)).Detail);
        var deep = FerruleBasicSourceRoots.Create(
            HierarchyInput(Text("r/" + string.Concat(Enumerable.Repeat("d/", 256)) + "leaf")),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), BasicLimits).RootContext();
        Equal<int?>(FerrulePathHierarchy.MaximumDepth, Error(FerruleRuntimeError.PathHierarchyDepth,
            () => OwnedHierarchy(deep, FerruleOwnedSourceOwner.Primary)).MaximumDepth);
    }
}
