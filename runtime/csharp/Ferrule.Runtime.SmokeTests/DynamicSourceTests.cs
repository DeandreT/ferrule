using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DynamicSourceContexts()
    {
        var source = Group(
            Field(
                "Properties",
                Group(
                    Field("wanted", Scalar(Text("outer"))),
                    Field("explicit-null", Scalar(FerruleValue.JsonNull)),
                    Field("structural", Group()))),
            Field(
                "Rows",
                Repeated(
                    Group(Field("Properties", Group())),
                    Group(Field("Other", Scalar(Text("unrelated")))))));
        var context = ScopeContext.FromSource(source);

        Equal(
            Text("outer"),
            context.ResolveDynamicScalar(null, new[] { "Properties" }, Text("wanted")));
        Equal(
            FerruleValue.JsonNull,
            context.ResolveDynamicScalar(null, new[] { "Properties" }, Text("explicit-null")));
        Equal(
            FerruleValue.Null,
            context.ResolveDynamicScalar(null, new[] { "Properties" }, Text("structural")));
        Equal(
            FerruleValue.Null,
            context.ResolveDynamicScalar(null, new[] { "Properties" }, Text("missing")));
        Equal(
            FerruleValue.Null,
            context.ResolveDynamicScalar(
                null,
                new[] { "Properties" },
                FerruleValue.FromInt64(1)));

        var rows = context.IterateSource("Rows");
        Equal(
            FerruleValue.Null,
            rows[0].ResolveDynamicScalar(null, new[] { "Properties" }, Text("wanted")));
        Equal(
            Text("outer"),
            rows[1].ResolveDynamicScalar(null, new[] { "Properties" }, Text("wanted")));
        Equal(
            FerruleValue.Null,
            rows[1].ResolveDynamicScalar(
                new[] { "Rows" },
                new[] { "Properties" },
                Text("wanted")));

        var named = ScopeContext.FromSources(
            Group(),
            new[]
            {
                Field(
                    "Settings",
                    Group(Field("selected", Scalar(Text("named"))))),
            });
        Equal(
            Text("named"),
            named.ResolveDynamicScalar(null, new[] { "Settings" }, Text("selected")));

        var drivers = ScopeContext.FromSource(Group(Field(
            "Files",
            Repeated(
                Group(Field("path", Scalar(Text("a.json")))),
                Group(Field("path", Scalar(FerruleValue.Null))),
                Group(Field("path", Scalar(Text("b.json"))))))));
        _ = Error(
            FerruleRuntimeError.MissingDynamicSourceLoader,
            () => FerruleDynamicSourceItems.Load(
                drivers,
                "Catalog",
                new[] { "Files" },
                new[] { "Rows" },
                7,
                candidate => candidate.ResolveScalar("path")));

        var loader = new DynamicLoader();
        var loaded = FerruleDynamicSourceItems.Load(
            drivers.WithDynamicSourceLoader(loader),
            "Catalog",
            new[] { "Files" },
            new[] { "Rows" },
            7,
            candidate => candidate.ResolveScalar("path"));
        Equal("a.json,b.json", string.Join(',', loader.Calls));
        var loadedContexts = loaded.Contexts();
        Equal(2, loadedContexts.Count);
        Equal(Text("a.json"), loadedContexts[0].ResolveScalar("path"));
        Equal(Text("loaded:b.json"), loadedContexts[1].ResolveScalar("value"));

        var invalid = ScopeContext.FromSource(
            Repeated(Scalar(FerruleValue.FromBoolean(true))))
            .WithDynamicSourceLoader(loader);
        var type = Error(
            FerruleRuntimeError.DynamicSourcePath,
            () => FerruleDynamicSourceItems.Load(
                invalid,
                "Catalog",
                Array.Empty<string>(),
                Array.Empty<string>(),
                9,
                candidate => candidate.ResolveScalar()));
        Equal((uint?)9, type.Node);
        Equal((FerruleValueKind?)FerruleValueKind.Bool, type.FoundKind);

        loader.Fail = true;
        var load = Error(
            FerruleRuntimeError.DynamicSourceLoad,
            () => FerruleDynamicSourceItems.Load(
                drivers.WithDynamicSourceLoader(loader),
                "Catalog",
                new[] { "Files" },
                new[] { "Rows" },
                7,
                candidate => candidate.ResolveScalar("path")));
        Equal("Catalog", load.SourceField);
        Equal("a.json", load.Detail);
    }

    private sealed class DynamicLoader : IFerruleDynamicSourceLoader
    {
        internal List<string> Calls { get; } = new();

        internal bool Fail { get; set; }

        public FerruleInstance Load(string sourceName, string logicalPath)
        {
            if (Fail)
            {
                throw new InvalidOperationException("fixture load failed");
            }
            Calls.Add(logicalPath);
            return Group(Field(
                "Rows",
                Repeated(Group(Field(
                    "value",
                    Scalar(Text($"loaded:{logicalPath}")))))));
        }
    }
}
