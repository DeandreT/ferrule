using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleGroup AdjacencyRow(string key, string? parent) =>
        Group(Field("Key", Scalar(Text(key))),
            Field("Parent", Scalar(parent is null ? FerruleValue.Null : Text(parent))));

    private static FerruleGroup AdjacencyInput(string root, params FerruleGroup[] rows) =>
        Group(Field("Root", Scalar(Text(root))),
            Field("Rows", Group(Field("Row", new FerruleRepeated(rows.Cast<FerruleInstance>())))));

    private static string AdjacencyName(FerruleInstance instance) =>
        ((FerruleScalar)((FerruleGroup)instance).Fields.Single(field => field.Name == "name").Value)
            .Value.StringValue;

    private static IReadOnlyList<FerruleInstance> AdjacencyChildren(FerruleInstance instance) =>
        ((FerruleRepeated)((FerruleGroup)instance).Fields.Single(field => field.Name == "children").Value)
            .Items;

    private static FerruleInstance OwnedAdjacency(FerruleBasicScopeContext context,
        FerruleOwnedSourceOwner owner, Func<FerruleBasicScopeContext, FerruleValue>? root = null) =>
        FerruleAdjacencyTree.BuildOwned(context, owner, new[] { "Rows", "Row" }, new[] { "Key" },
            new[] { "Parent" }, "name", "children", root);

    private static void AdjacencyTreeLegacyBuild()
    {
        var source = AdjacencyInput("r", AdjacencyRow("r", null), AdjacencyRow("b", "r"),
            AdjacencyRow("a", "r"), AdjacencyRow("x", "b"));
        var tree = FerruleAdjacencyTree.Build(ScopeContext.FromSource(source), new[] { "Rows", "Row" },
            new[] { "Key" }, new[] { "Parent" }, "name", "children", null);
        Equal("r", AdjacencyName(tree));
        Equal("b,a", string.Join(",", AdjacencyChildren(tree).Select(AdjacencyName)));
        Equal("x", AdjacencyName(AdjacencyChildren(AdjacencyChildren(tree)[0]).Single()));
        var missing = Error(FerruleRuntimeError.MissingAdjacencyCollection, () => FerrulePathRows(
            ScopeContext.FromSource(Group(Field("Other", Scalar(Text("x")))))));
        Equal("Rows/Row", missing.SourceField);
    }

    private static FerruleInstance FerrulePathRows(ScopeContext context) =>
        FerruleAdjacencyTree.Build(context, new[] { "Rows", "Row" }, new[] { "Key" },
            new[] { "Parent" }, "name", "children", null);

    private static void AdjacencyTreeOwnedEndpoints()
    {
        var primary = AdjacencyInput("p", AdjacencyRow("p", null), AdjacencyRow("p1", "p"));
        var audit = AdjacencyInput("n1", AdjacencyRow("n", null), AdjacencyRow("n1", "n"),
            AdjacencyRow("n2", "n1"));
        // A named endpoint spelled like a primary field never supplies the primary rows.
        var alias = AdjacencyInput("q", AdjacencyRow("q", null));
        var declared = new[]
        {
            new FerruleDeclaredNamedSourceRoot("audit", audit),
            new FerruleDeclaredNamedSourceRoot("Rows", alias),
            new FerruleDeclaredNamedSourceRoot("missing", null),
        };
        var context = FerruleBasicSourceRoots.Create(primary, declared, BasicLimits).RootContext();
        var tree = OwnedAdjacency(context, FerruleOwnedSourceOwner.Primary);
        Equal("p", AdjacencyName(tree));
        Equal("p1", AdjacencyName(AdjacencyChildren(tree).Single()));
        var named = OwnedAdjacency(context, FerruleOwnedSourceOwner.Named("audit"));
        Equal("n", AdjacencyName(named));
        Equal("n2", AdjacencyName(AdjacencyChildren(AdjacencyChildren(named).Single()).Single()));
        Equal("q", AdjacencyName(OwnedAdjacency(context, FerruleOwnedSourceOwner.Named("Rows"))));

        // The root expression reads the receiving context, not the selected rows:
        // the endpoint Root field selects a subtree of its rows.
        var rooted = OwnedAdjacency(context, FerruleOwnedSourceOwner.Named("audit"),
            parent => parent.ResolveOwnedField(FerruleOwnedSourceOwner.Named("audit"), new[] { "Root" }));
        Equal("n2", AdjacencyName(rooted));
        Equal(0, AdjacencyChildren(rooted).Count);

        var unavailable = Error(FerruleRuntimeError.OwnedSource,
            () => OwnedAdjacency(context, FerruleOwnedSourceOwner.Named("missing")));
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, unavailable.OwnedSourceFailure!.Error);

        // A supplied endpoint without rows has no root row.
        var empty = FerruleBasicSourceRoots.Create(Group(Field("Root", Scalar(Text("r")))),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), BasicLimits).RootContext();
        Equal("0", Error(FerruleRuntimeError.AdjacencyRootCount,
            () => OwnedAdjacency(empty, FerruleOwnedSourceOwner.Primary)).Detail);
        var duplicate = FerruleBasicSourceRoots.Create(
            AdjacencyInput("r", AdjacencyRow("r", null), AdjacencyRow("r", "r")),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), BasicLimits).RootContext();
        Equal("r", Error(FerruleRuntimeError.DuplicateAdjacencyKey,
            () => OwnedAdjacency(duplicate, FerruleOwnedSourceOwner.Primary)).Detail);
        var cycle = FerruleBasicSourceRoots.Create(AdjacencyInput("l", AdjacencyRow("l", "l")),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), BasicLimits).RootContext();
        Equal("l", Error(FerruleRuntimeError.AdjacencyCycle, () => OwnedAdjacency(cycle,
            FerruleOwnedSourceOwner.Primary, _ => Text("l"))).Detail);
        Equal("Int64", Error(FerruleRuntimeError.InvalidAdjacencyRoot, () => OwnedAdjacency(cycle,
            FerruleOwnedSourceOwner.Primary, _ => FerruleValue.FromInt64(1))).FoundKind.ToString());
    }
}
