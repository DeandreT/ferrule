using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string DynamicCatalogSchema =
        """{"name":"Catalog","kind":{"kind":"group","children":[{"name":"Rows","repeating":true,"kind":{"kind":"group","children":[{"name":"value","kind":{"kind":"scalar","ty":"string"}}]}}]}}""";

    private static ScopeContext DynamicDrivers(params string[] paths) =>
        ScopeContext.FromSource(Group(Field(
            "Files",
            Repeated(paths.Select(path => (FerruleInstance)Group(Field("path", Scalar(Text(path))))).ToArray()))));

    private static FerruleRuntimeException DynamicLoadError(ScopeContext drivers, IFerruleDynamicSourceLoader loader) =>
        Error(FerruleRuntimeError.DynamicSourceLoad, () => FerruleDynamicSourceItems.Load(
            drivers.WithDynamicSourceLoader(loader), "Catalog", new[] { "Files" }, new[] { "Rows" }, 7,
            candidate => candidate.ResolveScalar("path")));

    private static void DynamicSourceTaggedFailures()
    {
        // A legacy loader exception is retained unchanged as a host acquisition.
        var legacyCause = new IOException("share offline");
        var legacy = new ScriptedDynamicLoader("b.json", legacyCause);
        var legacyError = DynamicLoadError(DynamicDrivers("a.json", "b.json", "c.json"), legacy);
        Equal(true, ReferenceEquals(legacyCause, legacyError.InnerException));
        Equal("a.json,b.json", string.Join(',', legacy.Calls));

        // A tagged fault keeps its own identity and its original inner cause.
        var taggedCause = new IOException("share offline");
        var fault = FerruleDynamicSourceFault.ForAcquisition(taggedCause);
        var tagged = new ScriptedDynamicLoader("b.json", fault);
        var taggedError = DynamicLoadError(DynamicDrivers("a.json", "b.json", "c.json"), tagged);
        Equal(true, ReferenceEquals(fault, taggedError.InnerException));
        Equal(true, ReferenceEquals(taggedCause, fault.InnerException));

        // Old and new shapes project identically and keep the same request order.
        Equal(legacy.Calls.Count, tagged.Calls.Count);
        Equal(string.Join(',', legacy.Calls), string.Join(',', tagged.Calls));
        var expected = new FerruleDynamicSourceDiagnostic(FerruleDynamicSourcePhase.Acquisition, "Catalog",
            FerruleDynamicSourceCategory.Host, "b.json", null);
        Equal(expected, FerruleDynamicSourceDiagnostic.Find(legacyError));
        Equal(expected, FerruleDynamicSourceDiagnostic.Find(taggedError));
        Equal(legacyError.Message, taggedError.Message);

        // Request-phase failures precede every callback and path evaluation.
        var evaluated = 0;
        var missing = Error(FerruleRuntimeError.MissingDynamicSourceLoader, () => FerruleDynamicSourceItems.Load(
            DynamicDrivers("a.json"), "Catalog", new[] { "Files" }, Array.Empty<string>(), 3,
            _ => { evaluated++; return Text("a.json"); }));
        Equal(0, evaluated);
        Equal(new FerruleDynamicSourceDiagnostic(FerruleDynamicSourcePhase.Request, "Catalog",
            FerruleDynamicSourceCategory.MissingLoader, null, null), FerruleDynamicSourceDiagnostic.Find(missing));
        var never = new ScriptedDynamicLoader("", new InvalidOperationException("unused"));
        var longPath = Error(FerruleRuntimeError.DynamicSourcePathTooLong, () => FerruleDynamicSourceItems.Load(
            DynamicDrivers("a.json").WithDynamicSourceLoader(never), "Catalog", new[] { "Files" },
            Array.Empty<string>(), 3, _ => Text(new string('x', FerruleDynamicSourceLimits.MaximumPathBytes + 1))));
        Equal(0, never.Calls.Count);
        Equal("path_too_long", FerruleDynamicSourceDiagnostic.Find(longPath)!.CategoryToken);
    }

    private static void DynamicSourceJsonAdapterFailures()
    {
        string? Schema(string source) => source == "Catalog" ? DynamicCatalogSchema : null;
        FerruleDynamicSourceFault Fault(Action action)
        {
            try { action(); }
            catch (FerruleDynamicSourceFault fault) { return fault; }
            throw new InvalidOperationException("Expected a tagged dynamic-source fault.");
        }

        var host = new QueuedJsonLoader(
            Encoding.UTF8.GetBytes("""{"Rows":[{"value":"ok"}]}"""),
            new byte[] { (byte)'{', 0xff },
            Encoding.UTF8.GetBytes("""{"Rows":"""),
            Encoding.UTF8.GetBytes("{}"));
        var adapter = new FerruleJsonDynamicSourceAdapter(host, Schema);
        _ = adapter.Load("Catalog", "a.json");
        var encoding = Fault(() => adapter.Load("Catalog", "b.json"));
        Equal(FerruleDynamicSourceCategory.Encoding, encoding.Category);
        Equal(true, encoding.InnerException is DecoderFallbackException);
        Equal(true, encoding.Message.StartsWith("document is not UTF-8: ", StringComparison.Ordinal));
        var document = Fault(() => adapter.Load("Catalog", "c.json"));
        Equal(FerruleDynamicSourceCategory.Document, document.Category);
        Equal(FerruleRuntimeError.JsonBoundary, ((FerruleRuntimeException)document.InnerException!).Error);
        var undeclared = Fault(() => adapter.Load("Other", "d.json"));
        Equal(FerruleDynamicSourceCategory.UndeclaredSource, undeclared.Category);
        Equal("undeclared dynamic source 'Other'", undeclared.Message);
        Equal("a.json,b.json,c.json,d.json", string.Join(',', host.Calls));

        var limited = new FerruleJsonDynamicSourceAdapter(new QueuedJsonLoader(
            Encoding.UTF8.GetBytes("""{"Rows":[]}"""),
            new byte[17],
            Encoding.UTF8.GetBytes("""{"Rows":[]} """)), Schema, 16, 20);
        _ = limited.Load("Catalog", "a.json");
        var single = Fault(() => limited.Load("Catalog", "b.json"));
        Equal(new FerruleDynamicSourceResourceLimit(FerruleDynamicSourceResource.DocumentBytes, 17, 16), single.ResourceLimit);
        Equal("document exceeds the 16-byte dynamic-source limit", single.Message);
        var total = Fault(() => limited.Load("Catalog", "c.json"));
        Equal(new FerruleDynamicSourceResourceLimit(FerruleDynamicSourceResource.TotalBytes, 23, 20), total.ResourceLimit);
        Equal("documents exceed the 20-byte combined dynamic-source limit", total.Message);
        Equal<Exception?>(null, total.InnerException);

        // Host exceptions pass through the adapter unchanged.
        var hostCause = new UnauthorizedAccessException("denied");
        var failing = new FerruleJsonDynamicSourceAdapter(new QueuedJsonLoader(hostCause), Schema);
        var hostError = DynamicLoadError(DynamicDrivers("a.json"), failing);
        Equal(true, ReferenceEquals(hostCause, hostError.InnerException));
        var codecError = DynamicLoadError(DynamicDrivers("a.json"),
            new FerruleJsonDynamicSourceAdapter(new QueuedJsonLoader(new byte[] { 0xff }), Schema));
        Equal(new FerruleDynamicSourceDiagnostic(FerruleDynamicSourcePhase.Codec, "Catalog",
            FerruleDynamicSourceCategory.Encoding, "a.json", null), FerruleDynamicSourceDiagnostic.Find(codecError));
    }

    private static void DynamicSourceProjectionTokens()
    {
        Equal("request,acquisition,codec,resource", string.Join(',',
            Enum.GetValues<FerruleDynamicSourcePhase>().Select(FerruleDynamicSourceDiagnostic.Token)));
        Equal("missing_loader,path_type,path_too_long,too_many_requests,host,encoding,document," +
            "undeclared_source,document_bytes,total_bytes,request_count", string.Join(',',
            Enum.GetValues<FerruleDynamicSourceCategory>().Select(FerruleDynamicSourceDiagnostic.Token)));
        var tooMany = new FerruleRuntimeException(FerruleRuntimeError.DynamicSourceTooMany, "many", sourceField: "Catalog");
        Equal("request/too_many_requests", Tokens(FerruleDynamicSourceDiagnostic.FromRuntimeException(tooMany)!));
        Equal<FerruleDynamicSourceDiagnostic?>(null, FerruleDynamicSourceDiagnostic.FromRuntimeException(
            new FerruleRuntimeException(FerruleRuntimeError.MappingFailure, "rule")));
        var resource = new FerruleRuntimeException(FerruleRuntimeError.DynamicSourceLoad, "limit",
            FerruleDynamicSourceFault.ForResource(FerruleDynamicSourceResource.RequestCount, 2, 1),
            detail: "p", sourceField: "Catalog");
        var projected = FerruleDynamicSourceDiagnostic.Find(new InvalidOperationException("outer", resource))!;
        Equal("resource/request_count", Tokens(projected));
        Equal(new FerruleDynamicSourceResourceLimit(FerruleDynamicSourceResource.RequestCount, 2, 1), projected.Resource);

        // A recovered XML product refusal projects its owned request.
        var adapter = new FerruleXmlDynamicSourceAdapter(new DynamicMixedInvalidBytes(),
            new FerruleXmlDynamicSourcePolicy(1, "catalog", "{}"), new FerruleXmlInputSetBudget(3));
        var marker = CaptureDynamicMixedLoad(adapter);
        var recovered = adapter.Recover(DynamicMixedRuntime(marker, "catalog", "b.xml"));
        Equal(new FerruleDynamicSourceDiagnostic(FerruleDynamicSourcePhase.Codec, "catalog",
            FerruleDynamicSourceCategory.Encoding, "b.xml", null), FerruleDynamicSourceDiagnostic.Find(recovered));
        var undeclared = new FerruleXmlDynamicSourceAdapter(new DynamicMixedInvalidBytes(),
            new FerruleXmlDynamicSourcePolicy(1, "catalog", "{}"), new FerruleXmlInputSetBudget(3));
        Throws<FerruleDynamicSourceFault>(() => undeclared.Load("other", "b.xml"));
    }

    private static string Tokens(FerruleDynamicSourceDiagnostic diagnostic) =>
        $"{diagnostic.PhaseToken}/{diagnostic.CategoryToken}";

    private sealed class ScriptedDynamicLoader(string fail, Exception failure) : IFerruleDynamicSourceLoader
    {
        internal List<string> Calls { get; } = new();

        public FerruleInstance Load(string sourceName, string logicalPath)
        {
            Calls.Add(logicalPath);
            if (logicalPath == fail)
            {
                throw failure;
            }
            return Group(Field("Rows", Repeated(Group(Field("value", Scalar(Text(logicalPath)))))));
        }
    }

    private sealed class QueuedJsonLoader(params object[] responses) : IFerruleDynamicJsonSourceLoader
    {
        private readonly Queue<object> _responses = new(responses);

        internal List<string> Calls { get; } = new();

        public byte[] Load(string sourceName, string logicalPath)
        {
            Calls.Add(logicalPath);
            return _responses.Dequeue() switch
            {
                byte[] bytes => bytes,
                Exception error => throw error,
                _ => throw new InvalidOperationException("unexpected response"),
            };
        }
    }
}
