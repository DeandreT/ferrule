using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleGroup GeneratedEndpoint(string code, params string[] rows) => Group(
        Field("Code", Scalar(Text(code))),
        Field("Rows", new FerruleRepeated(rows.Select(row => (FerruleInstance)Group(Field("Code", Scalar(Text(row))))))));

    private static void BasicGeneratedItemsKeepExactOwnedReads()
    {
        var root = FerruleBasicSourceRoots.Create(GeneratedEndpoint("P", "r1", "r2"),
            new[] { new FerruleDeclaredNamedSourceRoot("Code", GeneratedEndpoint("N", "n1")) },
            new FerruleOwnedSourceLimits(8, 8, 16, 65_536, 32)).RootContext();
        var row = root.IterateOwnedSource(FerruleOwnedSourceOwner.Primary, new[] { "Rows" })[1].Context;
        var contexts = row.EnumerateGenerated(new[] { FerruleValue.FromInt64(10), Text("x") }, 77).ToArray();
        Equal(2, contexts.Length);
        var items = new[] { FerruleValue.FromInt64(10), Text("x") };
        for (var index = 0; index < contexts.Length; index++)
        {
            var context = contexts[index];
            Equal(items[index], context.ResolveLegacyField(Array.Empty<string>()));
            Equal(index + 1L, context.LegacyPosition(Array.Empty<string>()));
            Equal(FerruleValue.FromInt64(index + 1), context.GeneratedPosition(9, 77));
            Equal(Text("r2"), context.ResolveOwnedField(FerruleOwnedSourceOwner.Primary,
                new[] { "Rows", "Code" }, new[] { "Rows" }));
            Equal((nuint)2, context.OwnedPosition(FerruleOwnedSourceOwner.Primary, new[] { "Rows" }));
            // Exact named and primary roots keep their colliding field values.
            Equal(Text("N"), context.ResolveOwnedFieldWithRootPin(FerruleOwnedSourceOwner.Named("Code"),
                new[] { "Code" }, Array.Empty<string>(), true));
            Equal(Text("P"), context.ResolveOwnedFieldWithRootPin(FerruleOwnedSourceOwner.Primary,
                new[] { "Code" }, Array.Empty<string>(), true));
            // An unpinned ordinary read still falls back outward past the item.
            Equal(Text("r2"), context.ResolveLegacyField(new[] { "Code" }));
        }
        // The receiving view is unchanged.
        Equal(2L, row.LegacyPosition(Array.Empty<string>()));
        Error(FerruleRuntimeError.GeneratedPositionUnavailable, () => row.GeneratedPosition(9,77));
        Equal(Text("r2"), row.ResolveLegacyField(new[] { "Code" }));
    }

    private static void BasicGeneratedItemsAreLazyAndMissingOwnersFailWhenRead()
    {
        var root = FerruleBasicSourceRoots.Create(GeneratedEndpoint("P"),
            new[] { new FerruleDeclaredNamedSourceRoot("Code", null) },
            new FerruleOwnedSourceLimits(8, 8, 16, 65_536, 32)).RootContext();
        Equal(0, root.EnumerateGenerated(Array.Empty<FerruleValue>()).Count());
        Throws<ArgumentNullException>(() => root.EnumerateGenerated(null!));
        // An existential consumer stops before later items allocate a view.
        using var contexts = root.EnumerateGenerated(new[] { Bool(true), Bool(false) }).GetEnumerator();
        Equal(true, contexts.MoveNext());
        var context = contexts.Current;
        var error = Error(FerruleRuntimeError.OwnedSource, () => context.ResolveOwnedFieldWithRootPin(
            FerruleOwnedSourceOwner.Named("Code"), new[] { "Code" }, Array.Empty<string>(), true));
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, error.OwnedSourceFailure!.Error);
        error = Error(FerruleRuntimeError.OwnedSource, () => context.OwnedPosition(
            FerruleOwnedSourceOwner.Primary, new[] { "Rows" }));
        Equal(FerruleOwnedSourceErrorKind.InactiveSourceFrame, error.OwnedSourceFailure!.Error);
    }
}
