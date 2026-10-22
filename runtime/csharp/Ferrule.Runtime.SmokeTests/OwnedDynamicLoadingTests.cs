using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private sealed class RecordingDocuments : IFerruleDynamicSourceLoader
    {
        internal List<string> Calls { get; } = new();
        public FerruleInstance Load(string sourceName, string logicalPath)
        {
            Calls.Add($"{sourceName}:{logicalPath}");
            return (sourceName, logicalPath) switch
            {
                ("Docs", "a.json") => LoadedDoc("A", new[] { "a1", "a2" }, new[] { "p1.json" }),
                ("Docs", "b.json") => LoadedDoc("B", new[] { "b1" }, new[] { "p1.json", "p2.json" }),
                ("Docs", "c.json") => LoadedDoc("C", Array.Empty<string>(), Array.Empty<string>()),
                ("Parts", "p1.json") => Group(Field("Name", Scalar(Text("P1")))),
                ("Parts", "p2.json") => Group(Field("Name", Scalar(Text("P2")))),
                _ => throw new InvalidOperationException($"no document {sourceName}:{logicalPath}"),
            };
        }
    }

    private static FerruleGroup LoadedDoc(string title, string[] lines, string[] parts) => Group(
        Field("Title", Scalar(Text(title))),
        Field("Lines", new FerruleRepeated(lines.Select(code => (FerruleInstance)Group(Field("Code", Scalar(Text(code))))).ToArray())),
        Field("Parts", new FerruleRepeated(parts.Select(path => (FerruleInstance)Group(Field("Path", Scalar(Text(path))))).ToArray())));

    private static FerruleGroup DriverBatch(params (FerruleValue Path, string Label)[] rows) => Group(
        Field("Files", new FerruleRepeated(rows.Select(row => (FerruleInstance)Group(
            Field("Path", Scalar(row.Path)), Field("Label", Scalar(Text(row.Label))))).ToArray())));

    private static List<List<FerruleValue>> LoadedRows(FerruleBasicScopeContext context, FerruleOwnedSourceOwner driver)
    {
        var docs = FerruleOwnedSourceOwner.Named("Docs");
        var parts = FerruleOwnedSourceOwner.Named("Parts");
        var files = new[] { "Files" };
        var documents = context.LoadOwnedDynamicDocuments("Docs", driver, files, 1,
            row => row.ResolveOwnedField(driver, new[] { "Files", "Path" }, files));
        var loading = documents.LoadingContext(context);
        var rows = new List<List<FerruleValue>>();
        foreach (var candidate in loading.WalkOwnedSourceCohort(docs, Array.Empty<string>()))
        {
            var item = candidate.Context;
            var row = new List<FerruleValue>
            {
                item.ResolveOwnedField(docs, new[] { "Title" }),
                item.ResolveOwnedField(driver, new[] { "Files", "Label" }, files),
                FerruleValue.FromInt64((long)item.OwnedPosition(driver, files)),
            };
            foreach (var line in item.WalkOwnedSourceCohort(docs, new[] { "Lines" }))
            {
                row.Add(line.Context.ResolveOwnedField(docs, new[] { "Lines", "Code" }, new[] { "Lines" }));
                row.Add(FerruleValue.FromInt64((long)line.Context.OwnedPosition(docs, new[] { "Lines" })));
            }
            // Nested loading reads only this document's Parts rows.
            var nested = item.LoadOwnedDynamicDocuments("Parts", docs, new[] { "Parts" }, 2,
                part => part.ResolveOwnedField(docs, new[] { "Parts", "Path" }, new[] { "Parts" }));
            foreach (var part in nested.LoadingContext(item).WalkOwnedSourceCohort(parts, Array.Empty<string>()))
            {
                row.Add(part.Context.ResolveOwnedField(parts, new[] { "Name" }));
                row.Add(part.Context.ResolveOwnedFieldWithRootPin(docs, new[] { "Title" }, Array.Empty<string>(), true));
                row.Add(part.Context.ResolveOwnedField(driver, new[] { "Files", "Label" }, files));
            }
            rows.Add(row);
        }
        return rows;
    }

    private static void SequenceEqual(IReadOnlyList<FerruleValue> expected, IReadOnlyList<FerruleValue> actual)
    {
        Equal(expected.Count, actual.Count);
        for (var i = 0; i < expected.Count; i++) Equal(expected[i], actual[i]);
    }

    private static void OwnedDynamicLoadingDocuments()
    {
        var primary = DriverBatch((Text("a.json"), "first"), (Text("b.json"), "second"));
        var catalog = DriverBatch((Text("c.json"), "catalog"));
        var declared = new[]
        {
            new FerruleDeclaredNamedSourceRoot("Catalog", catalog),
            new FerruleDeclaredNamedSourceRoot("Docs", null),
            new FerruleDeclaredNamedSourceRoot("Parts", null),
        };
        var loader = new RecordingDocuments();
        var roots = FerruleBasicSourceRoots.Create(primary, declared, BasicLimits).WithDynamicSourceLoader(loader);
        var context = roots.RootContext();
        var rows = LoadedRows(context, FerruleOwnedSourceOwner.Primary);
        Equal(2, rows.Count);
        SequenceEqual(new[] { Text("A"), Text("first"), FerruleValue.FromInt64(1), Text("a1"),
            FerruleValue.FromInt64(1), Text("a2"), FerruleValue.FromInt64(2), Text("P1"), Text("A"), Text("first") }, rows[0]);
        SequenceEqual(new[] { Text("B"), Text("second"), FerruleValue.FromInt64(2), Text("b1"),
            FerruleValue.FromInt64(1), Text("P1"), Text("B"), Text("second"), Text("P2"), Text("B"), Text("second") }, rows[1]);
        // Every reached path is one callback; generated code never caches.
        SequenceEqual(new[] { "Docs:a.json", "Docs:b.json", "Parts:p1.json", "Parts:p1.json", "Parts:p2.json" }
            .Select(Text).ToArray(), loader.Calls.Select(Text).ToArray());
        // Outside its loading scope the dynamic owner has no root.
        var outside = Error(FerruleRuntimeError.OwnedSource,
            () => context.ResolveOwnedField(FerruleOwnedSourceOwner.Named("Docs"), new[] { "Title" }));
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, outside.OwnedSourceFailure!.Error);

        // The same-shaped named driver supplies only its own rows.
        var catalogLoader = new RecordingDocuments();
        var catalogRows = LoadedRows(FerruleBasicSourceRoots.Create(primary, declared, BasicLimits)
            .WithDynamicSourceLoader(catalogLoader).RootContext(), FerruleOwnedSourceOwner.Named("Catalog"));
        Equal(1, catalogRows.Count);
        SequenceEqual(new[] { Text("C"), Text("catalog"), FerruleValue.FromInt64(1) }, catalogRows[0]);
        SequenceEqual(new[] { Text("Docs:c.json") }, catalogLoader.Calls.Select(Text).ToArray());
    }

    private static void OwnedDynamicLoadingFailures()
    {
        FerruleDeclaredNamedSourceRoot[] Declared(FerruleInstance? catalog) => new[]
        {
            new FerruleDeclaredNamedSourceRoot("Catalog", catalog),
            new FerruleDeclaredNamedSourceRoot("Docs", null),
            new FerruleDeclaredNamedSourceRoot("Parts", null),
        };
        // A missing loader fails before the driver walk, even when it is empty.
        var missing = Error(FerruleRuntimeError.MissingDynamicSourceLoader,
            () => LoadedRows(FerruleBasicSourceRoots.Create(DriverBatch(), Declared(null), BasicLimits).RootContext(),
                FerruleOwnedSourceOwner.Primary));
        Equal("Docs", missing.SourceField);
        // An unsupplied driver endpoint is never repaired by Primary's rows.
        var loader = new RecordingDocuments();
        var unsupplied = Error(FerruleRuntimeError.OwnedSource,
            () => LoadedRows(FerruleBasicSourceRoots.Create(DriverBatch((Text("a.json"), "first")), Declared(null), BasicLimits)
                .WithDynamicSourceLoader(loader).RootContext(), FerruleOwnedSourceOwner.Named("Catalog")));
        Equal(FerruleOwnedSourceErrorKind.SourceUnavailable, unsupplied.OwnedSourceFailure!.Error);
        Equal("Catalog", unsupplied.OwnedSourceFailure!.Owner?.Name);
        Equal(0, loader.Calls.Count);
        // Absent and null paths skip; another value stops before later drivers.
        loader = new RecordingDocuments();
        var mixed = DriverBatch((FerruleValue.Null, "skip"), (FerruleValue.JsonNull, "skip"), (Text("c.json"), "kept"),
            (FerruleValue.FromInt64(7), "bad"), (Text("a.json"), "late"));
        var path = Error(FerruleRuntimeError.DynamicSourcePath,
            () => LoadedRows(FerruleBasicSourceRoots.Create(mixed, Declared(null), BasicLimits)
                .WithDynamicSourceLoader(loader).RootContext(), FerruleOwnedSourceOwner.Primary));
        Equal<uint?>(1, path.Node);
        SequenceEqual(new[] { Text("Docs:c.json") }, loader.Calls.Select(Text).ToArray());
        // A host failure keeps its source and logical path.
        loader = new RecordingDocuments();
        var failed = Error(FerruleRuntimeError.DynamicSourceLoad,
            () => LoadedRows(FerruleBasicSourceRoots.Create(DriverBatch((Text("a.json"), "first"), (Text("missing.json"), "second")),
                Declared(null), BasicLimits).WithDynamicSourceLoader(loader).RootContext(), FerruleOwnedSourceOwner.Primary));
        Equal("missing.json", failed.Detail);
        Equal("Docs", failed.SourceField);
        SequenceEqual(new[] { Text("Docs:a.json"), Text("Docs:missing.json") }, loader.Calls.Select(Text).ToArray());
    }
}
