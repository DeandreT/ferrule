using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleGroup OwnedProps(params (string Name, FerruleValue Value)[] properties) =>
        Group(properties.Select(property => Field(property.Name, Scalar(property.Value))).ToArray());

    private static void OwnedDynamicFieldEndpoints()
    {
        // Primary and Lookup share every object and key name with different values.
        var primary = Group(
            Field("Props", OwnedProps(("color", Text("P.red")), ("blank", FerruleValue.JsonNull))),
            Field("Rows", new FerruleRepeated(new FerruleInstance[]
            {
                Group(Field("Props", OwnedProps(("color", Text("P.row1"))))),
                Group(Field("Props", OwnedProps(("size", Text("P.row2"))))),
            })));
        var lookup = Group(
            Field("Props", OwnedProps(("size", Text("N.small")))),
            Field("Rows", new FerruleRepeated(new FerruleInstance[]
            {
                Group(Field("Props", OwnedProps(("color", Text("N.row1"))))),
            })));
        var declared = new[]
        {
            new FerruleDeclaredNamedSourceRoot("Lookup", lookup),
            new FerruleDeclaredNamedSourceRoot("Settings", OwnedProps(("mode", Text("S.fast")))),
            new FerruleDeclaredNamedSourceRoot("Missing", null),
        };
        var context = FerruleBasicSourceRoots.Create(primary, declared, BasicLimits).RootContext();
        var props = new[] { "Props" };
        var primaryOwner = FerruleOwnedSourceOwner.Primary;
        var lookupOwner = FerruleOwnedSourceOwner.Named("Lookup");
        FerruleValue Read(FerruleBasicScopeContext scope, FerruleOwnedSourceOwner owner, string[] path,
            string[]? frame, FerruleValue key, bool nonrepeating = true) =>
            scope.ResolveOwnedDynamicField(owner, path, frame, key, nonrepeating);

        Equal(Text("P.red"), Read(context, primaryOwner, props, null, Text("color")));
        // A property missing from the selected owner never borrows Primary's.
        Equal(FerruleValue.Null, Read(context, lookupOwner, props, null, Text("color")));
        Equal(Text("N.small"), Read(context, lookupOwner, props, null, Text("size")));
        Equal(FerruleValue.JsonNull, Read(context, primaryOwner, props, null, Text("blank")));
        // Non-string keys read nothing, including from an unsupplied owner.
        Equal(FerruleValue.Null, Read(context, FerruleOwnedSourceOwner.Named("Missing"), props, null,
            FerruleValue.FromInt64(1)));
        // An open endpoint root and an explicit root pin.
        Equal(Text("S.fast"), Read(context, FerruleOwnedSourceOwner.Named("Settings"),
            Array.Empty<string>(), null, Text("mode")));
        Equal(Text("P.red"), Read(context, primaryOwner, props, Array.Empty<string>(), Text("color")));

        var rows = new[] { "Rows" };
        var rowProps = new[] { "Rows", "Props" };
        var primaryRows = context.IterateOwnedSource(primaryOwner, rows);
        Equal(2, primaryRows.Count);
        Equal(Text("P.row1"), Read(primaryRows[0].Context, primaryOwner, rowProps, null, Text("color")));
        Equal(Text("P.row2"), Read(primaryRows[1].Context, primaryOwner, rowProps, rows, Text("size")));
        // The Primary row frame is never consulted for the Lookup owner, which
        // has no active row: its uniterated repetition is refused.
        var uniterated = Error(FerruleRuntimeError.OwnedSource,
            () => Read(primaryRows[0].Context, lookupOwner, rowProps, null, Text("color")));
        Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, uniterated.OwnedSourceFailure!.Error);
        var inactive = Error(FerruleRuntimeError.OwnedSource,
            () => Read(primaryRows[0].Context, lookupOwner, rowProps, rows, Text("color")));
        Equal(FerruleOwnedSourceErrorKind.InactiveSourceFrame, inactive.OwnedSourceFailure!.Error);
        var lookupRows = context.IterateOwnedSource(lookupOwner, rows);
        Equal(Text("N.row1"), Read(lookupRows[0].Context, lookupOwner, rowProps, null, Text("color")));

        var unavailable = Error(FerruleRuntimeError.OwnedSource,
            () => Read(context, FerruleOwnedSourceOwner.Named("Missing"), props, null, Text("color")));
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, unavailable.OwnedSourceFailure!.Error);
        Equal<uint?>(7, unavailable.WithOwnedSourceNode(7).Node);
        var incompatible = Error(FerruleRuntimeError.OwnedSource,
            () => Read(context, primaryOwner, props, rows, Text("color")));
        Equal(FerruleOwnedSourceErrorKind.IncompatibleFramePath, incompatible.OwnedSourceFailure!.Error);
        Throws<ArgumentNullException>(() => context.ResolveOwnedDynamicField(primaryOwner, null!, null,
            Text("color"), true));
    }
}
