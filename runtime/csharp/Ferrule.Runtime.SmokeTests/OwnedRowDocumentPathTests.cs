using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static FerruleGroup RowPathGroup(string path, params FerruleField[] fields) =>
        Group(fields).WithSourceOrigin(new FerruleSourceDocumentOrigin(path).Row(1));

    private static FerruleOwnedSourceException RowPathError(Action read)
    {
        try { read(); }
        catch (FerruleOwnedSourceException error) { return error; }
        throw new Exception("Expected owned row metadata failure.");
    }

    private static void OwnedRowDocumentPaths()
    {
        var primary = RowPathGroup("p.root.csv",
            Field("Ref", RowPathGroup("collision.csv")),
            Field("Rows", new FerruleRepeated([RowPathGroup("p.row.csv",
                Field("Children", new FerruleRepeated([Group(), RowPathGroup("p.child.csv")])))])));
        var named = RowPathGroup("n.root.csv", Field("Rows", new FerruleRepeated([RowPathGroup("n.row.csv")])));
        var limits = new FerruleOwnedSourceLimits(8, 16, 16, 1048576, 64);
        var roots = FerruleOwnedSourceRoots.Create(primary,
            [new("Ref", named), new("Absent", null)], limits);
        var root = roots.RootContext();
        var p = FerruleOwnedSourceOwner.Primary;
        var n = FerruleOwnedSourceOwner.Named("Ref");
        Equal(Text("p.root.csv"), root.RowDocumentPath(p));
        Equal(Text("n.root.csv"), root.RowDocumentPath(n));
        Equal(FerruleOwnedSourceErrorKind.InactiveSourceFrame, RowPathError(() => root.RowDocumentPath(p, ["Rows"])).Error);
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, RowPathError(() => root.RowDocumentPath(FerruleOwnedSourceOwner.Named("Absent"))).Error);
        var primaryRow = root.IterateSource(p, ["Rows"])[0];
        var both = primaryRow.IterateSource(n, ["Rows"])[0];
        Equal(Text("p.row.csv"), both.RowDocumentPath(p));
        Equal(Text("n.row.csv"), both.RowDocumentPath(n));
        Equal(Text("p.root.csv"), both.RowDocumentPath(p, []));
        Equal(Text("n.root.csv"), both.RowDocumentPath(n, []));
        var children = both.IterateSource(p, ["Rows", "Children"]);
        var missing = RowPathError(() => children[0].RowDocumentPath(p));
        Equal(FerruleOwnedSourceErrorKind.MissingRowDocumentPath, missing.Error);
        Equal(true, missing.Owner!.IsPrimary);
        Equal(true, missing.Collection is null);
        missing = RowPathError(() => children[0].RowDocumentPath(p, ["Rows", "Children"]));
        Equal("Rows/Children", string.Join('/', missing.Collection!));
        Equal(Text("p.row.csv"), children[0].RowDocumentPath(p, ["Rows"]));
        Equal(Text("n.row.csv"), children[0].RowDocumentPath(n));
        Equal(Text("p.child.csv"), children[1].RowDocumentPath(p));
        var basic = FerruleBasicSourceRoots.Create(primary, [new("Ref", named)], limits).RootContext();
        var basicPrimaryRow = basic.IterateOwnedSource(p, ["Rows"])[0].Context;
        var basicBoth = basicPrimaryRow.IterateOwnedSource(n, ["Rows"])[0].Context;
        Equal(Text("p.row.csv"), basicBoth.OwnedRowDocumentPath(p));
        Equal(Text("n.row.csv"), basicBoth.OwnedRowDocumentPath(n));
        Equal(Text("p.root.csv"), basicBoth.OwnedRowDocumentPath(p, []));
        var basicChildren = basicBoth.IterateOwnedSource(p, ["Rows", "Children"]);
        try
        {
            basicChildren[0].Context.OwnedRowDocumentPath(p);
            throw new Exception("Expected missing paired row metadata.");
        }
        catch (FerruleRuntimeException error)
        {
            Equal(FerruleRuntimeError.OwnedSource, error.Error);
            var cause = (FerruleOwnedSourceException)error.InnerException!;
            Equal(FerruleOwnedSourceErrorKind.MissingRowDocumentPath, cause.Error);
            Equal(true, cause.Owner!.IsPrimary);
            Equal(true, cause.Collection is null);
        }
        Equal(Text("p.child.csv"), basicChildren[1].Context.OwnedRowDocumentPath(p));
        var repeatedRoot = FerruleOwnedSourceRoots.Create(new FerruleRepeated([RowPathGroup("row.csv")]), [], limits).RootContext();
        var row = repeatedRoot.IterateSource(p, [])[0];
        Equal(Text("row.csv"), row.RowDocumentPath(p));
        Equal(FerruleOwnedSourceErrorKind.MissingRowDocumentPath, RowPathError(() => row.RowDocumentPath(p, [])).Error);
    }
}
