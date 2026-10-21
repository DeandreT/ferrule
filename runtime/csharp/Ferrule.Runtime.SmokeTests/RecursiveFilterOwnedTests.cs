using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static readonly FerruleOwnedSourceOwner FilterLookup = FerruleOwnedSourceOwner.Named("Lookup");

    private static FerruleGroup FilterFile(string name, long pos, long dirpos, string dir) =>
        Group(Field("name", Scalar(Text(name))), Field("pos", Scalar(FerruleValue.FromInt64(pos))),
            Field("dirpos", Scalar(FerruleValue.FromInt64(dirpos))), Field("dir", Scalar(Text(dir))));

    private static FerruleGroup FilterDirectory(string name, FerruleInstance[] files, params FerruleInstance[] children) =>
        Group(Field("name", Scalar(Text(name))), Field("file", new FerruleRepeated(files)),
            Field("Dir", new FerruleRepeated(children)));

    private static FerruleGroup FilterInput(string suffix, params FerruleInstance[] directories) =>
        Group(Field("Suffix", Scalar(Text(suffix))), Field("Dir", new FerruleRepeated(directories)));

    private static FerruleOwnedSourceLimits FilterLimits => new(8, 8, 300, 1_048_576, 64);

    private static string FilterNames(FerruleInstance instance, string collection) =>
        string.Join(",", ((FerruleRepeated)((FerruleGroup)instance).Fields.Single(field => field.Name == collection).Value)
            .Items.Select(item => ((FerruleScalar)((FerruleGroup)item).Fields.Single(field => field.Name == "name").Value)
                .Value.StringValue));

    private static IReadOnlyList<FerruleInstance> FilterChildren(FerruleInstance instance) =>
        ((FerruleRepeated)((FerruleGroup)instance).Fields.Single(field => field.Name == "Dir").Value).Items;

    private static List<FerruleInstance> FilterEach(FerruleBasicScopeContext context,
        Func<FerruleBasicScopeContext, FerruleValue> predicate) =>
        context.WalkOwnedSourceCohort(FilterLookup, new[] { "Dir" })
            .Select(candidate => FerruleRecursiveFilter.ApplyOwned(candidate.Context, FilterLookup,
                new[] { "Dir" }, "Dir", "file", 9, predicate))
            .ToList();

    private static void RecursiveFilterOwnedEndpoints()
    {
        var primary = FilterInput(".keep", FilterDirectory("p", Array.Empty<FerruleInstance>()));
        var lookup = FilterInput(".lk",
            FilterDirectory("a", new FerruleInstance[]
                {
                    FilterFile("x.keep", 1, 1, "a"), FilterFile("y.lk", 9, 1, "a"), FilterFile("z.keep", 3, 1, "a"),
                },
                FilterDirectory("b", new FerruleInstance[] { FilterFile("b.keep", 1, 1, "b"), FilterFile("c.keep", 2, 9, "b") },
                    FilterDirectory("c", new FerruleInstance[] { FilterFile("d.keep", 1, 1, "a") })),
                FilterDirectory("d", new FerruleInstance[] { FilterFile("e.keep", 1, 2, "d") })),
            FilterDirectory("e", new FerruleInstance[] { FilterFile("f.keep", 1, 2, "e") }));
        var declared = new[] { new FerruleDeclaredNamedSourceRoot("Lookup", lookup) };
        var context = FerruleBasicSourceRoots.Create(primary, declared, FilterLimits).RootContext();

        // Lookup files ending with the Primary endpoint's suffix.
        var suffixed = FilterEach(context, item =>
        {
            var name = item.ResolveOwnedField(FilterLookup, new[] { "Dir", "file", "name" });
            var suffix = item.ResolveOwnedField(FerruleOwnedSourceOwner.Primary, new[] { "Suffix" });
            return Bool(name.StringValue.EndsWith(suffix.StringValue, StringComparison.Ordinal));
        });
        Equal("x.keep,z.keep", FilterNames(suffixed[0], "file"));
        Equal("b.keep,c.keep", FilterNames(FilterChildren(suffixed[0])[0], "file"));
        Equal("e.keep", FilterNames(FilterChildren(suffixed[0])[1], "file"));
        Equal("f.keep", FilterNames(suffixed[1], "file"));

        // Item and group positions plus the innermost group's name.
        var positioned = FilterEach(context, item =>
        {
            var pos = item.OwnedPosition(FilterLookup, new[] { "Dir", "file" });
            var dirpos = item.OwnedPosition(FilterLookup, new[] { "Dir" });
            var latest = item.OwnedPosition(FilterLookup);
            FerruleValue Read(params string[] path) => item.ResolveOwnedField(FilterLookup, path);
            return Bool(Read("Dir", "file", "pos").Int64Value == (long)pos
                && Read("Dir", "file", "dirpos").Int64Value == (long)dirpos
                && Read("Dir", "name").StringValue == Read("Dir", "file", "dir").StringValue
                && latest == pos);
        });
        Equal("x.keep,z.keep", FilterNames(positioned[0], "file"));
        Equal("b.keep", FilterNames(FilterChildren(positioned[0])[0], "file"));
        Equal(string.Empty, FilterNames(FilterChildren(FilterChildren(positioned[0])[0])[0], "file"));
        Equal("e.keep", FilterNames(FilterChildren(positioned[0])[1], "file"));
        Equal("f.keep", FilterNames(positioned[1], "file"));

        var missing = FerruleBasicSourceRoots.Create(primary,
            new[] { new FerruleDeclaredNamedSourceRoot("Lookup", null) }, FilterLimits).RootContext();
        var unavailable = Error(FerruleRuntimeError.OwnedSource, () => FerruleRecursiveFilter.ApplyOwned(missing,
            FilterLookup, Array.Empty<string>(), "Dir", "file", 9, _ => Bool(true)));
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, unavailable.OwnedSourceFailure!.Error);
    }

    private static void RecursiveFilterOwnedRootsAndErrors()
    {
        var primary = FilterInput(string.Empty);
        var tree = FilterDirectory("t", new FerruleInstance[] { FilterFile("t.keep", 1, 1, "t") });
        FerruleBasicScopeContext Root(FerruleInstance lookup) => FerruleBasicSourceRoots.Create(primary,
            new[] { new FerruleDeclaredNamedSourceRoot("Lookup", lookup) }, FilterLimits).RootContext();
        FerruleInstance Apply(FerruleBasicScopeContext context, string[] collection,
            Func<FerruleBasicScopeContext, FerruleValue> predicate) =>
            FerruleRecursiveFilter.ApplyOwned(context, FilterLookup, collection, "Dir", "file", 9, predicate);

        // A named Group root is one identity group without an occurrence.
        Equal("t.keep", FilterNames(Apply(Root(tree), Array.Empty<string>(), _ => Bool(true)), "file"));
        Equal("missing context", Error(FerruleRuntimeError.RecursiveFilterRequiresGroup,
            () => Apply(Root(tree), new[] { "Dir" }, _ => Bool(true))).FoundInstance);
        Error(FerruleRuntimeError.NotABool, () => Apply(Root(tree), Array.Empty<string>(), _ => Text("no")));
        var malformed = Error(FerruleRuntimeError.RecursiveFilterRequiresCollection,
            () => Apply(Root(Group(Field("file", Scalar(Text("x"))))), Array.Empty<string>(), _ => Bool(true)));
        Equal("file", malformed.SourceField);

        FerruleInstance Nest(int levels)
        {
            FerruleInstance directory = FilterDirectory("leaf", Array.Empty<FerruleInstance>());
            for (var level = 0; level < levels; level++)
                directory = FilterDirectory("n", Array.Empty<FerruleInstance>(), directory);
            return directory;
        }
        Apply(Root(Nest(FerruleRecursiveFilter.MaximumDepth - 1)), Array.Empty<string>(), _ => Bool(true));
        Error(FerruleRuntimeError.RecursiveFilterDepth,
            () => Apply(Root(Nest(FerruleRecursiveFilter.MaximumDepth)), Array.Empty<string>(), _ => Bool(true)));
    }
}
