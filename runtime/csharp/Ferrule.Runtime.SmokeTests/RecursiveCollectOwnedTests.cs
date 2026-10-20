using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleGroup OwnedDirectory(string note, string[] files, params FerruleInstance[] children) =>
        Group(
            Field("Note", Scalar(Text(note))),
            Field("file", new FerruleRepeated(files.Select(file => (FerruleInstance)Group(Field("Note", Scalar(Text(file))))))),
            Field("Dir", new FerruleRepeated(children)));

    private static FerruleGroup OwnedForest(string note, params FerruleInstance[] directories) =>
        Group(Field("Note", Scalar(Text(note))), Field("Dir", new FerruleRepeated(directories)));

    private static IReadOnlyList<FerruleValue> OwnedCollect(FerruleBasicScopeContext context,
        FerruleOwnedSourceOwner owner, string[] collection) =>
        FerruleSequences.RecursiveCollectOwned(context, owner, collection, new[] { "Dir" }, new[] { "Note" },
            new[] { "file" }, new[] { "Note" }, string.Empty, "/");

    private static void RecursiveCollectOwnedEndpoints()
    {
        var primary = OwnedForest("P.note", OwnedDirectory("p", new[] { "a" }, OwnedDirectory("sub", new[] { "b" })));
        var lookup = OwnedForest("N.note", OwnedDirectory("n", new[] { "x", "y" }));
        var tree = OwnedDirectory("t", new[] { "root.txt" });
        var declared = new[]
        {
            new FerruleDeclaredNamedSourceRoot("Lookup", lookup),
            new FerruleDeclaredNamedSourceRoot("Tree", tree),
            new FerruleDeclaredNamedSourceRoot("Empty", Group(Field("Note", Scalar(Text("E.note"))))),
            new FerruleDeclaredNamedSourceRoot("Missing", null),
        };
        var context = FerruleBasicSourceRoots.Create(primary, declared, BasicLimits).RootContext();
        var dir = new[] { "Dir" };
        Equal("/p/a,/p/sub/b", string.Join(",", OwnedCollect(context, FerruleOwnedSourceOwner.Primary, dir).Select(ValueText)));
        Equal("/n/x,/n/y", string.Join(",", OwnedCollect(context, FerruleOwnedSourceOwner.Named("Lookup"), dir).Select(ValueText)));
        // The endpoint root is one identity group; relative Note paths never read an endpoint field.
        Equal("/t/root.txt", string.Join(",", OwnedCollect(context, FerruleOwnedSourceOwner.Named("Tree"),
            Array.Empty<string>()).Select(ValueText)));
        // An absent selected collection is empty; the Primary tree never supplies it.
        Equal(0, OwnedCollect(context, FerruleOwnedSourceOwner.Named("Empty"), dir).Count);

        var unavailable = Error(FerruleRuntimeError.OwnedSource,
            () => OwnedCollect(context, FerruleOwnedSourceOwner.Named("Missing"), dir));
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, unavailable.OwnedSourceFailure!.Error);
        Equal<uint?>(null, unavailable.Node);
        Equal<uint?>(7, unavailable.WithOwnedSourceNode(7).Node);

        var nullLeaf = FerruleBasicSourceRoots.Create(OwnedForest("P", Group(
                Field("Note", Scalar(Text("d"))),
                Field("file", new FerruleRepeated(new FerruleInstance[] { Group(Field("Note", Scalar(FerruleValue.Null))) })))),
            Array.Empty<FerruleDeclaredNamedSourceRoot>(), BasicLimits).RootContext();
        Error(FerruleRuntimeError.FunctionType, () => OwnedCollect(nullLeaf, FerruleOwnedSourceOwner.Primary, dir));

        FerruleInstance deepest = OwnedDirectory("leaf", Array.Empty<string>());
        for (var depth = 0; depth < 256; depth++)
        {
            deepest = OwnedDirectory($"level-{depth}", Array.Empty<string>(), deepest);
        }
        var deep = FerruleBasicSourceRoots.Create(OwnedForest("P", deepest), Array.Empty<FerruleDeclaredNamedSourceRoot>(),
            BasicLimits).RootContext();
        Error(FerruleRuntimeError.RecursiveSequenceDepth, () => OwnedCollect(deep, FerruleOwnedSourceOwner.Primary, dir));
        Throws<ArgumentNullException>(() => FerruleSequences.RecursiveCollectOwned(context,
            FerruleOwnedSourceOwner.Primary, new string[] { null! }, dir, dir, dir, dir, string.Empty, "/"));
    }
}
