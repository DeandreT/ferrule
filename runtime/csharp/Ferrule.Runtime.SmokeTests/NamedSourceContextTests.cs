using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void NamedSourceContexts()
    {
        var catalog = Group(Field(
            "Customers",
            Repeated(
                Customer(1, "Ada"),
                Customer(2, "Lin"))));
        var settings = Group(Field("Prefix", Scalar(Text("extra"))));
        var primary = Group(
            Field("settings", Group(Field("Prefix", Scalar(Text("primary"))))),
            Field("Needle", Scalar(FerruleValue.FromInt64(2))));
        var execution = new FerruleExecutionContext("mapping.ferrule");
        var context = ScopeContext.FromSources(
            primary,
            new[]
            {
                Field("catalog", catalog),
                Field("settings", settings),
            },
            execution);

        Equal(2, context.Frames.Count);
        Equal("catalog,settings", string.Join(',', ((FerruleGroup)context.Frames[0]).Fields.Select(
            field => field.Name)));
        Equal(Text("primary"), context.ResolveScalar("settings", "Prefix"));
        Equal(Text("Ada"), context.ResolveScalar("catalog", "Customers", "Name"));
        Equal(Text("mapping.ferrule"), context.ResolveRuntimeValue(FerruleRuntimeValue.MappingFilePath));

        var customers = context.IterateSource("catalog", "Customers");
        Equal(2, customers.Count);
        Equal(Text("Ada"), customers[0].ResolveScalar("Name"));
        Equal(Text("Lin"), customers[1].ResolveScalar("Name"));

        var aggregate = context.AggregateItems("catalog", "Customers");
        Equal(2, aggregate.Count);
        Equal(Text("Lin"), aggregate[1].AggregateCurrentScalar("Name"));
        Equal(
            Text("Lin"),
            context.Lookup(
                new[] { "catalog", "Customers" },
                new[] { "Id" },
                FerruleValue.FromInt64(2),
                new[] { "Name" }));

        Equal(1, ScopeContext.FromSource(primary).Frames.Count);
        Error(
            FerruleRuntimeError.DuplicateField,
            () => ScopeContext.FromSources(
                primary,
                new[]
                {
                    Field("catalog", catalog),
                    Field("catalog", catalog),
                }));
        Throws<ArgumentNullException>(() => ScopeContext.FromSources(null!, Array.Empty<FerruleField>()));
        Throws<ArgumentNullException>(() => ScopeContext.FromSources(primary, null!));
    }

    private static FerruleGroup Customer(long id, string name) =>
        Group(
            Field("Id", Scalar(FerruleValue.FromInt64(id))),
            Field("Name", Scalar(Text(name))));
}

internal static partial class Program
{
    private static FerruleOwnedSourceLimits OwnedLimits => new(32, 32, 32, 65_536, 64);
    private static FerruleOwnedSourceOwner OwnedPrimaryOwner => FerruleOwnedSourceOwner.Primary;
    private static FerruleOwnedSourceOwner OwnedNamedOwner(string name) => FerruleOwnedSourceOwner.Named(name);
    private static FerruleRepeated OwnedRepeated(params FerruleInstance[] items) => new(items);
    private static FerruleOwnedScopeContext OwnedCollision(FerruleOwnedSourceLimits? limits = null)
    {
        var primary = Group(Field("Catalog", Group(Field("Label", Scalar(Text("primary"))))),
            Field("Value", Scalar(Text("root"))),
            Field("Parents", OwnedRepeated(
                Group(Field("Value", Scalar(Text("parent-1"))), Field("Children", OwnedRepeated(
                    Group(Field("Value", Scalar(Text("child-a")))), Group()))),
                Group(Field("Value", Scalar(Text("parent-2"))), Field("Children", OwnedRepeated(
                    Group(Field("Value", Scalar(Text("child-c"))))))))));
        return FerruleOwnedSourceRoots.Create(primary, new[]
        {
            new FerruleDeclaredNamedSourceRoot("Catalog", OwnedRepeated(
                Group(Field("Label", Scalar(Text("named-1"))), Field("Value", Scalar(Text("named-1")))),
                Group(Field("Label", Scalar(Text("named-2"))), Field("Value", Scalar(Text("named-2")))))),
            new FerruleDeclaredNamedSourceRoot("Mirror", Group(Field("Parents", OwnedRepeated(
                Group(Field("Value", Scalar(Text("mirror-parent"))), Field("Children", OwnedRepeated(
                    Group(Field("Value", Scalar(Text("mirror-child"))))))))))),
            new FerruleDeclaredNamedSourceRoot(" Catalog ", Group(Field("Value", Scalar(Text("spaced"))))),
            new FerruleDeclaredNamedSourceRoot("catalog", Group(Field("Value", Scalar(Text("lower"))))),
            new FerruleDeclaredNamedSourceRoot("Absent", null),
        }, limits ?? OwnedLimits).RootContext();
    }
    private static void OwnedRaw(string id, object observation) =>
        Console.WriteLine(System.Text.Json.JsonSerializer.Serialize(new { id, observation }));
    private static void OwnedValue(string id, FerruleValue expected, FerruleValue actual)
    {
        OwnedRaw(id, new { kind = actual.Kind.ToString(), value = actual.Kind switch
        { FerruleValueKind.String => (object?)actual.StringValue, FerruleValueKind.Int64 => actual.Int64Value,
          FerruleValueKind.Bool => actual.BooleanValue, FerruleValueKind.Double => actual.DoubleValue, _ => null } });
        Equal(expected, actual);
    }
    private static void OwnedIndex(string id, nuint expected, nuint actual)
    { OwnedRaw(id, new { index = (ulong)actual }); Equal(expected, actual); }
    private static void OwnedPath(IReadOnlyList<string> expected, IReadOnlyList<string>? actual)
    {
        Equal(false, actual is null); Equal(expected.Count, actual!.Count);
        for (var i = 0; i < expected.Count; i++) Equal(expected[i], actual[i]);
    }
    private static FerruleOwnedSourceException OwnedError(string id, FerruleOwnedSourceErrorKind expected, Action action)
    {
        try { action(); }
        catch (FerruleOwnedSourceException e)
        {
            var cause = e.InnerException as FerruleOwnedSourceTraversalException;
            OwnedRaw(id, new { error = e.Error.ToString(), owner = e.Owner?.Name,
                primary = e.Owner?.IsPrimary, name = e.RequestedName,
                declarations = e.DeclarationIndices, path = e.Path, frame = e.Frame,
                collection = e.Collection, segment = e.Segment.HasValue ? (ulong?)e.Segment.Value : null,
                found = e.FoundInstance?.ToString(), index = e.RequestedIndex.HasValue ? (ulong?)e.RequestedIndex.Value : null,
                limit = e.LimitKind?.ToString(), used = e.Used.HasValue ? (ulong?)e.Used.Value : null,
                requested = e.Requested.HasValue ? (ulong?)e.Requested.Value : null,
                maximum = e.Maximum.HasValue ? (ulong?)e.Maximum.Value : null,
                innerType = e.InnerException?.GetType().FullName,
                cause = cause is null ? null : new { error = cause.Error.ToString(), path = cause.Path,
                    segment = cause.Segment.HasValue ? (ulong?)cause.Segment.Value : null, found = cause.Found.ToString() } });
            Equal(expected, e.Error); return e;
        }
        throw new InvalidOperationException("Expected owned source refusal.");
    }
    private static IReadOnlyList<FerruleOwnedScopeContext> OwnedRows(string id, IReadOnlyList<FerruleOwnedScopeContext> actual)
    {
        OwnedRaw(id, new { count = actual.Count, registryIdentities = actual.Select(c => System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(c.Roots)).ToArray(), frames = actual.Select(c => Enumerable.Range(0, c.FrameCount)
            .Select(i => { var f = c.FrameAt(i); return new { owner = c.Roots.Name(f.Owner),
                primary = f.Owner == 0, declarationIndex = f.Owner == 0 ? (int?)null : f.Owner - 1, path = f.Path, index = (ulong)f.Index,
                itemKind = f.Item.GetType().FullName,
                itemIdentity = System.Runtime.CompilerServices.RuntimeHelpers.GetHashCode(f.Item) }; }).ToArray()).ToArray() });
        return actual;
    }
    private static void OwnedRuntimeNamed()
    {
        var c = OwnedCollision();
        OwnedValue("R01.primary", Text("primary"), c.ResolveField(OwnedPrimaryOwner, new[] { "Catalog", "Label" }));
        var repeated = OwnedError("R01.named", FerruleOwnedSourceErrorKind.UniteratedRepetition,
            () => c.ResolveField(OwnedNamedOwner("Catalog"), new[] { "Label" }));
        Equal("Catalog", repeated.Owner!.Name); OwnedPath(new[] { "Label" }, repeated.Path); Equal<nuint?>(0, repeated.Segment);
        var named = OwnedRows("R02.setup", c.IterateSource(OwnedNamedOwner("Catalog"), Array.Empty<string>()));
        Equal(2, named.Count);
        OwnedValue("R02.named", Text("named-2"), named[1].ResolveField(OwnedNamedOwner("Catalog"), new[] { "Label" }));
        OwnedValue("R02.primary", Text("primary"), named[1].ResolveField(OwnedPrimaryOwner, new[] { "Catalog", "Label" }));
        OwnedValue("R03.spaced", Text("spaced"), c.ResolveField(OwnedNamedOwner(" Catalog "), new[] { "Value" }));
        OwnedValue("R03.lower", Text("lower"), c.ResolveField(OwnedNamedOwner("catalog"), new[] { "Value" }));
        Equal("CATALOG", OwnedError("R03.unknown", FerruleOwnedSourceErrorKind.UnknownOwner,
            () => c.ResolveField(OwnedNamedOwner("CATALOG"), new[] { "Value" })).RequestedName);
        var ambiguous = FerruleOwnedSourceRoots.Create(Group(Field("Value", Scalar(Text("primary-ok")))),
            new[] { new FerruleDeclaredNamedSourceRoot("Duplicate", Group(Field("Value", Scalar(Text("left"))))),
                    new FerruleDeclaredNamedSourceRoot("Duplicate", null) }, OwnedLimits).RootContext();
        var duplicate = OwnedError("R04", FerruleOwnedSourceErrorKind.AmbiguousOwner,
            () => ambiguous.ResolveField(OwnedNamedOwner("Duplicate"), new[] { "Value" }));
        Equal("Duplicate", duplicate.RequestedName); Equal(2, duplicate.DeclarationIndices!.Count);
        Equal(0, duplicate.DeclarationIndices[0]); Equal(1, duplicate.DeclarationIndices[1]);
        OwnedValue("R05", Text("primary-ok"), ambiguous.ResolveField(OwnedPrimaryOwner, new[] { "Value" }));
        OwnedError("R06.empty", FerruleOwnedSourceErrorKind.InvalidNamedOwner, () => c.ResolveField(OwnedNamedOwner(""), new[] { "Value" }));
        Equal("Unknown", OwnedError("R06.unknown", FerruleOwnedSourceErrorKind.UnknownOwner,
            () => c.ResolveField(OwnedNamedOwner("Unknown"), new[] { "Value" })).RequestedName);
        Equal("Absent", OwnedError("R06.unavailable", FerruleOwnedSourceErrorKind.SourceUnavailable,
            () => c.ResolveField(OwnedNamedOwner("Absent"), new[] { "Value" })).RequestedName);
        OwnedValue("R07", Text("root"), c.ResolveField(OwnedPrimaryOwner, new[] { "Value" }));
        var mirror = OwnedRows("R10.setup", c.IterateSource(OwnedNamedOwner("Mirror"), new[] { "Parents", "Children" }))[0];
        var foreign = OwnedError("R10.primary", FerruleOwnedSourceErrorKind.UniteratedRepetition,
            () => mirror.ResolveField(OwnedPrimaryOwner, new[] { "Parents", "Children", "Value" }));
        Equal(true, foreign.Owner!.IsPrimary); Equal<nuint?>(1, foreign.Segment);
        OwnedPath(new[] { "Parents", "Children", "Value" }, foreign.Path);
        OwnedValue("R10.mirror", Text("mirror-child"), mirror.ResolveField(OwnedNamedOwner("Mirror"), new[] { "Parents", "Children", "Value" }));
        var outerMirror = OwnedRows("R17.setup", c.IterateSource(OwnedNamedOwner("Mirror"), new[] { "Parents" }))[0];
        var inactive = OwnedError("R17.primary", FerruleOwnedSourceErrorKind.InactiveSourceFrame,
            () => outerMirror.Position(OwnedPrimaryOwner, new[] { "Parents" }));
        Equal(true, inactive.Owner!.IsPrimary); OwnedPath(new[] { "Parents" }, inactive.Collection);
        OwnedIndex("R17.mirror", 1, outerMirror.Position(OwnedNamedOwner("Mirror"), new[] { "Parents" }));
        var caller = new[] { new FerruleDeclaredNamedSourceRoot("Keep 雪 ", Group(Field("Value", Scalar(Text("kept"))))) };
        var saved = FerruleOwnedSourceRoots.Create(Group(Field("Value", Scalar(Text("primary")))), caller, OwnedLimits).RootContext();
        caller[0] = new("Other", Group(Field("Value", Scalar(Text("replaced")))));
        OwnedValue("CS01.kept", Text("kept"), saved.ResolveField(OwnedNamedOwner("Keep 雪 "), new[] { "Value" }));
        OwnedError("CS01.other", FerruleOwnedSourceErrorKind.UnknownOwner, () => saved.ResolveField(OwnedNamedOwner("Other"), new[] { "Value" }));
    }
}
