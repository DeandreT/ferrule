using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleGroup OwnedXmlInput(string header, params FerruleInstance[] rows) => Group(
        Field("Header", Group(Field("id", Scalar(Text(header))))),
        Field("Rows", new FerruleRepeated(rows)));

    private static FerruleGroup OwnedXmlRow(string code) =>
        Group(Field("Detail", Group(Field("code", Scalar(Text(code))))));

    private static string OwnedXmlId(FerruleInstance element) =>
        ((FerruleScalar)((FerruleGroup)element).Fields.Single(field => field.Name == "id").Value)
            .Value.StringValue;

    private static FerruleOwnedSourceException OwnedXmlFailure(Func<FerruleInstance> read) =>
        Error(FerruleRuntimeError.OwnedSource, () => read()).OwnedSourceFailure!;

    private static void OwnedXmlExactEndpoints()
    {
        var primary = OwnedXmlInput("p", OwnedXmlRow("a"), Group());
        // A named endpoint spelled like the primary field never supplies the primary element.
        var alias = OwnedXmlInput("h");
        var declared = new[] { new FerruleDeclaredNamedSourceRoot("Header", alias) };
        var context = FerruleBasicSourceRoots.Create(primary, declared, BasicLimits).RootContext();
        var header = new[] { "Header" };
        var detail = new[] { "Rows", "Detail" };
        Equal("p", OwnedXmlId(context.ResolveOwnedXmlInstance(FerruleOwnedSourceOwner.Primary, header, null, false)));
        Equal("h", OwnedXmlId(context.ResolveOwnedXmlInstance(FerruleOwnedSourceOwner.Named("Header"), header, null, false)));
        Equal(FerruleOwnedSourceErrorKind.UniteratedRepetition, OwnedXmlFailure(
            () => context.ResolveOwnedXmlInstance(FerruleOwnedSourceOwner.Primary, detail, null, false)).Error);

        var rows = context.IterateOwnedSource(FerruleOwnedSourceOwner.Primary, new[] { "Rows" });
        var first = rows[0].Context.ResolveOwnedXmlInstance(FerruleOwnedSourceOwner.Primary, detail, new[] { "Rows" }, false);
        Equal("a", ((FerruleScalar)((FerruleGroup)first).Fields.Single().Value).Value.StringValue);
        // The second row's absent element never borrows the first row's.
        var absent = OwnedXmlFailure(
            () => rows[1].Context.ResolveOwnedXmlInstance(FerruleOwnedSourceOwner.Primary, detail, null, false));
        Equal(FerruleOwnedSourceErrorKind.Traversal, absent.Error);
        Equal(FerruleOwnedSourceTraversalKind.MissingInstance,
            ((FerruleOwnedSourceTraversalException)absent.InnerException!).Error);
        Equal("Rows/Detail", string.Join('/', absent.Path!));
        // An explicit root pin reads the declared nonrepeating endpoint root.
        Equal("p", OwnedXmlId(rows[1].Context.ResolveOwnedXmlInstance(
            FerruleOwnedSourceOwner.Primary, header, Array.Empty<string>(), true)));
        Equal(FerruleOwnedSourceErrorKind.InactiveSourceFrame, OwnedXmlFailure(
            () => rows[1].Context.ResolveOwnedXmlInstance(
                FerruleOwnedSourceOwner.Named("Header"), detail, new[] { "Rows" }, false)).Error);
    }
}
