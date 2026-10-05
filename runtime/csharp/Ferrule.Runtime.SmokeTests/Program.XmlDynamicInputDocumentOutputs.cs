using Ferrule.Runtime;
namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void XmlDynamicInputDocumentOutputsOwners()
    {
        var decoder = new System.Text.DecoderFallbackException("original decoder");
        var schema = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Schema, "original descriptor", decoder);
        var source = FerruleXmlInputSource.Named(2, "unused");
        var input = FerruleXmlDynamicInputDocumentOutputsExecutionException.FromInputBoundary(source, schema);
        Equal<FerruleXmlDynamicInputDocumentOutputsOwner?>(new FerruleXmlDynamicInputDocumentOutputsOwner.Input(source), input.Owner);
        Equal<FerruleXmlDynamicInputRequest?>(null, input.Request);
        Equal(true, ReferenceEquals(schema, input.Boundary));
        Equal(true, ReferenceEquals(schema, input.InnerException));
        Equal(true, ReferenceEquals(decoder, input.Boundary.InnerException));
        var writer = new InvalidOperationException("original writer");
        var boundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Output, writer.Message, writer);
        foreach (var original in new[] {
            FerruleXmlDocumentOutputsExecutionException.PrimarySerialization(boundary),
            FerruleXmlDocumentOutputsExecutionException.NamedSerialization(1, "beta", 1, "../雪.xml", boundary),
        })
        {
            var error = FerruleXmlDynamicInputDocumentOutputsExecutionException.FromOutputs(original);
            Equal<FerruleXmlDynamicInputDocumentOutputsOwner?>(new FerruleXmlDynamicInputDocumentOutputsOwner.Output(original.Owner!), error.Owner);
            Equal(true, ReferenceEquals(original.Owner, ((FerruleXmlDynamicInputDocumentOutputsOwner.Output)error.Owner!).Owner));
            Equal(true, ReferenceEquals(boundary, error.Boundary));
            Equal(true, ReferenceEquals(boundary, error.InnerException));
            Equal(true, ReferenceEquals(writer, error.Boundary.InnerException));
            Equal<FerruleXmlDynamicInputRequest?>(null, error.Request);
        }
        foreach (var original in new[] {
            FerruleXmlDocumentOutputsExecutionException.PrimarySerialization(schema),
            FerruleXmlDocumentOutputsExecutionException.NamedSerialization(1, "beta", 1, "same.xml", schema),
            FerruleXmlDocumentOutputsExecutionException.Alignment("incomplete envelopes"),
            CaptureDynamicMixedOutput(() => _ = new FerruleXmlDocumentOutputsBudget(4097)),
        })
        {
            var error = FerruleXmlDynamicInputDocumentOutputsExecutionException.FromOutputs(original);
            Equal<FerruleXmlDynamicInputDocumentOutputsOwner?>(null, error.Owner);
            Equal<FerruleXmlDynamicInputRequest?>(null, error.Request);
            Equal(true, ReferenceEquals(original.Boundary, error.InnerException));
            Equal(true, ReferenceEquals(original.Boundary.InnerException, error.Boundary.InnerException));
        }
    }

    private static void XmlDynamicInputDocumentOutputsAdapterChannels()
    {
        var adapter = new FerruleXmlDynamicSourceAdapter(new DynamicMixedInvalidBytes(),
            new FerruleXmlDynamicSourcePolicy(1, "catalog", "{}"), new FerruleXmlInputSetBudget(3));
        var marker = CaptureDynamicMixedLoad(adapter);
        var runtime = DynamicMixedRuntime(marker, "catalog", "b.xml");
        var original = adapter.Recover(runtime);
        Equal(FerruleXmlBoundaryErrorKind.Utf8, original.Boundary.Kind);
        var request = original.Request!;
        Equal(1, request.DeclarationIndex);
        Equal("catalog", request.Source);
        Equal("b.xml", request.Path);
        Equal(1UL, request.Ordinal);
        Equal(true, request.CallbackInvoked);
        var error = FerruleXmlDynamicInputDocumentOutputsExecutionException.FromDynamicInputBoundary(request, original.Boundary);
        Equal<FerruleXmlDynamicInputDocumentOutputsOwner?>(new FerruleXmlDynamicInputDocumentOutputsOwner.Input(FerruleXmlInputSource.Named(1, "catalog")), error.Owner);
        Equal(true, ReferenceEquals(request, error.Request));
        Equal(true, ReferenceEquals(original.Boundary, error.Boundary));
        Equal(true, ReferenceEquals(original.Boundary, error.InnerException));
        Equal(true, ReferenceEquals(original.Boundary.InnerException, error.Boundary.InnerException));
        Equal(true, error.Boundary.InnerException is System.Text.DecoderFallbackException);

        // Each mismatch owns a fresh terminal adapter; no failed-load retry.
        for (var mismatch = 0; mismatch < 3; mismatch++)
        {
            var fresh = new FerruleXmlDynamicSourceAdapter(new DynamicMixedInvalidBytes(),
                new FerruleXmlDynamicSourcePolicy(1, "catalog", "{}"), new FerruleXmlInputSetBudget(3));
            var originalMarker = CaptureDynamicMixedLoad(fresh);
            var suppliedMarker = mismatch == 2 ? new InvalidOperationException(originalMarker.Message, originalMarker.InnerException) : originalMarker;
            var typed = DynamicMixedRuntime(suppliedMarker, mismatch == 0 ? "other" : "catalog", mismatch == 1 ? "other.xml" : "b.xml");
            var refusedRecovery = fresh.Recover(typed);
            Equal<FerruleXmlInputSource?>(null, refusedRecovery.Input);
            Equal<FerruleXmlDynamicInputRequest?>(null, refusedRecovery.Request);
            var global = FerruleXmlDynamicInputDocumentOutputsExecutionException.FromBoundary(refusedRecovery.Boundary);
            Equal<FerruleXmlDynamicInputDocumentOutputsOwner?>(null, global.Owner);
            Equal<FerruleXmlDynamicInputRequest?>(null, global.Request);
            Equal(FerruleXmlBoundaryErrorKind.Mapping, global.Boundary.Kind);
            Equal(true, ReferenceEquals(typed, global.Boundary.InnerException));
            Equal(true, ReferenceEquals(suppliedMarker, typed.InnerException));
            GC.KeepAlive(originalMarker);
        }

        var hostBoundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Utf8, "original host XML error", new System.Text.DecoderFallbackException("host decoder"));
        var hostAdapter = new FerruleXmlDynamicSourceAdapter(new DynamicMixedHostError(hostBoundary),
            new FerruleXmlDynamicSourcePolicy(1, "catalog", "{}"), new FerruleXmlInputSetBudget(3));
        var hostMarker = CaptureDynamicMixedLoad(hostAdapter);
        Equal(true, ReferenceEquals(hostBoundary, hostMarker));
        var hostRuntime = DynamicMixedRuntime(hostMarker, "catalog", "b.xml");
        var recoveredHost = hostAdapter.Recover(hostRuntime);
        var hostError = FerruleXmlDynamicInputDocumentOutputsExecutionException.FromBoundary(recoveredHost.Boundary);
        Equal<FerruleXmlDynamicInputDocumentOutputsOwner?>(null, hostError.Owner);
        Equal<FerruleXmlDynamicInputRequest?>(null, hostError.Request);
        Equal(FerruleXmlBoundaryErrorKind.Mapping, hostError.Boundary.Kind);
        Equal(true, ReferenceEquals(hostRuntime, hostError.Boundary.InnerException));
        Equal(true, ReferenceEquals(hostBoundary, hostError.Boundary.InnerException!.InnerException));
        XmlDynamicInputDocumentOutputsMultipleSourceChannels();
    }

    private static void XmlDynamicInputDocumentOutputsIndependentLedgers()
    {
        // Counter-only resource controls, not large input or serialized-output allocations.
        var inputBudget = new FerruleXmlInputSetBudget(3);
        inputBudget.Charge(FerruleXmlInputSource.Primary, 64L * 1024 * 1024);
        inputBudget.Charge(FerruleXmlInputSource.Named(0, "rates"), 64L * 1024 * 1024);
        inputBudget.Charge(FerruleXmlInputSource.Named(2, "unused"), 64L * 1024 * 1024);
        inputBudget.Reserve(FerruleXmlInputSource.Named(1, "catalog"));
        inputBudget.Charge(FerruleXmlInputSource.Named(1, "catalog"), 64L * 1024 * 1024);
        var inputOriginal = CaptureDynamicMixedInput(() => inputBudget.Charge(FerruleXmlInputSource.Named(1, "catalog"), 1));
        var inputError = FerruleXmlDynamicInputDocumentOutputsExecutionException.FromInputBoundary(inputOriginal.Input, inputOriginal.Boundary);
        Equal<FerruleXmlDynamicInputDocumentOutputsOwner?>(new FerruleXmlDynamicInputDocumentOutputsOwner.Input(FerruleXmlInputSource.Named(1, "catalog")), inputError.Owner);
        Equal<FerruleXmlDynamicInputRequest?>(null, inputError.Request);
        Equal(true, ReferenceEquals(inputOriginal.Boundary, inputError.Boundary));
        var inputCause = (FerruleXmlInputSetResourceException)inputError.Boundary.InnerException!;
        Equal("xml_input_set_utf8_bytes", inputCause.Resource);
        Equal(256UL * 1024 * 1024 + 1, inputCause.ObservedCount);
        var outputBudget = new FerruleXmlDocumentOutputsBudget(5);
        outputBudget.ChargePrimary(64L * 1024 * 1024);
        outputBudget.ChargeNamed(0, "alpha", 0, "same.xml", 64L * 1024 * 1024);
        outputBudget.ChargeNamed(1, "beta", 0, "same.xml", 64L * 1024 * 1024);
        outputBudget.ChargeNamed(1, "beta", 1, "/opaque.xml", 64L * 1024 * 1024);
        var outputOriginal = CaptureDynamicMixedOutput(() => outputBudget.ChargeNamed(1, "beta", 2, "../last.xml", 1));
        var outputError = FerruleXmlDynamicInputDocumentOutputsExecutionException.FromOutputs(outputOriginal);
        Equal<FerruleXmlDynamicInputDocumentOutputsOwner?>(new FerruleXmlDynamicInputDocumentOutputsOwner.Output(new FerruleXmlDocumentOutputsOwner.NamedMember(1, "beta", 2, "../last.xml")), outputError.Owner);
        Equal<FerruleXmlDynamicInputRequest?>(null, outputError.Request);
        Equal(true, ReferenceEquals(outputOriginal.Boundary, outputError.Boundary));
        var outputCause = (FerruleXmlOutputSetResourceException)outputError.Boundary.InnerException!;
        Equal("xml_output_set_utf8_bytes", outputCause.Resource);
        Equal(256UL * 1024 * 1024 + 1, outputCause.ObservedCount);
    }

    private const string DynamicMixedFloatSchema = """{"name":"Catalog","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[{"name":"Amount","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"float"}}]}}""";
    private const string DynamicMixedIntSchema = """{"name":"Catalog","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[{"name":"Amount","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"int"}}]}}""";
    private static FerruleXmlDynamicSourcePolicy[] DynamicMixedMultiplePolicies() => new[] {
        new FerruleXmlDynamicSourcePolicy(1, "alpha", DynamicMixedFloatSchema),
        new FerruleXmlDynamicSourcePolicy(3, "beta", DynamicMixedIntSchema),
    };
    private static void XmlDynamicInputDocumentOutputsMultipleSourceChannels()
    {
        var successfulHost = new DynamicMixedMultipleBytes(false);
        var successful = FerruleXmlDynamicSourceAdapter.ForSources(successfulHost,
            DynamicMixedMultiplePolicies(), new FerruleXmlInputSetBudget(3));
        var alpha = (FerruleGroup)successful.Load("alpha", "same.xml");
        var beta = (FerruleGroup)successful.Load("beta", "same.xml");
        Equal(true, alpha.TryGetField("Amount", out var alphaAmount));
        Equal(true, beta.TryGetField("Amount", out var betaAmount));
        Equal(FerruleValueKind.Double, ((FerruleScalar)alphaAmount!).Value.Kind);
        Equal(FerruleValueKind.Int64, ((FerruleScalar)betaAmount!).Value.Kind);
        Equal(7.0, ((FerruleScalar)alphaAmount!).Value.DoubleValue);
        Equal(7L, ((FerruleScalar)betaAmount!).Value.Int64Value);
        Equal(2, successfulHost.Calls.Count);
        Equal(("alpha", "same.xml"), successfulHost.Calls[0]);
        Equal(("beta", "same.xml"), successfulHost.Calls[1]);

        // Counter-only resource modes; each actual original callback buffer is tiny.
        for (var mode = 0; mode < 3; mode++)
        {
            var host = new DynamicMixedMultipleBytes(mode == 0);
            var budget = new FerruleXmlInputSetBudget(mode == 1 ? 4095 : 3);
            if (mode == 2)
                budget.Charge(FerruleXmlInputSource.Primary, 256L * 1024 * 1024 - DynamicMixedMultipleBytes.Document.Length);
            var adapter = FerruleXmlDynamicSourceAdapter.ForSources(host, DynamicMixedMultiplePolicies(), budget);
            _ = adapter.Load("alpha", "same.xml");
            var marker = CaptureDynamicMixedMultipleLoad(adapter);
            var original = adapter.Recover(DynamicMixedRuntime(marker, "beta", "same.xml"));
            var request = original.Request!;
            var error = FerruleXmlDynamicInputDocumentOutputsExecutionException.FromDynamicInputBoundary(request, original.Boundary);
            Equal<FerruleXmlDynamicInputDocumentOutputsOwner?>(new FerruleXmlDynamicInputDocumentOutputsOwner.Input(FerruleXmlInputSource.Named(3, "beta")), error.Owner);
            Equal(3, request.DeclarationIndex);
            Equal("beta", request.Source);
            Equal("same.xml", request.Path);
            Equal(2UL, request.Ordinal);
            Equal(mode != 1, request.CallbackInvoked);
            Equal(true, ReferenceEquals(request, error.Request));
            Equal(true, ReferenceEquals(original.Boundary, error.Boundary));
            Equal(true, ReferenceEquals(original.Boundary, error.InnerException));
            Equal(true, ReferenceEquals(original.Boundary.InnerException, error.Boundary.InnerException));
            Equal(mode == 1 ? 1 : 2, host.Calls.Count);
            if (mode == 0)
            {
                Equal(FerruleXmlBoundaryErrorKind.Utf8, error.Boundary.Kind);
                Equal(true, error.Boundary.InnerException is System.Text.DecoderFallbackException);
            }
            else
            {
                Equal(FerruleXmlBoundaryErrorKind.Input, error.Boundary.Kind);
                var resource = (FerruleXmlInputSetResourceException)error.Boundary.InnerException!;
                Equal(mode == 1 ? "xml_input_artifact_count" : "xml_input_set_utf8_bytes", resource.Resource);
                Equal(mode == 1 ? 4097UL : 256UL * 1024 * 1024 + (ulong)DynamicMixedMultipleBytes.Document.Length, resource.ObservedCount);
                Equal(mode == 1 ? 4096UL : 256UL * 1024 * 1024, resource.Limit);
            }
            // Recovery is terminal; the next control creates a fresh adapter.
        }
    }
    private sealed class DynamicMixedMultipleBytes(bool invalidBeta) : IFerruleDynamicXmlSourceLoader
    {
        public static readonly byte[] Document = System.Text.Encoding.UTF8.GetBytes("<Catalog><Amount>7</Amount></Catalog>");
        public readonly List<(string Source, string Path)> Calls = new();
        public byte[] Load(string sourceName, string logicalPath)
        {
            Calls.Add((sourceName, logicalPath));
            return invalidBeta && sourceName == "beta" ? new byte[] { 0xff } : (byte[])Document.Clone();
        }
    }
    private static Exception CaptureDynamicMixedMultipleLoad(FerruleXmlDynamicSourceAdapter adapter)
    {
        try { _ = adapter.Load("beta", "same.xml"); }
        catch (Exception error) { return error; }
        throw new InvalidOperationException("Expected one terminal second-source refusal.");
    }

    private sealed class DynamicMixedInvalidBytes : IFerruleDynamicXmlSourceLoader
    {
        public byte[] Load(string sourceName, string logicalPath) => new byte[] { 0xff };
    }
    private sealed class DynamicMixedHostError(FerruleXmlBoundaryException boundary) : IFerruleDynamicXmlSourceLoader
    {
        public byte[] Load(string sourceName, string logicalPath) => throw boundary;
    }
    private static FerruleRuntimeException DynamicMixedRuntime(Exception marker, string source, string path) =>
        new(FerruleRuntimeError.DynamicSourceLoad, "original typed loader failure", marker, detail: path, sourceField: source);
    private static Exception CaptureDynamicMixedLoad(FerruleXmlDynamicSourceAdapter adapter)
    {
        try { _ = adapter.Load("catalog", "b.xml"); }
        catch (Exception error) { return error; }
        throw new InvalidOperationException("Expected one terminal adapter load failure.");
    }
    private static FerruleXmlExecutionException CaptureDynamicMixedInput(Action action)
    {
        try { action(); }
        catch (FerruleXmlExecutionException error) { return error; }
        throw new InvalidOperationException("Expected trusted input admission failure.");
    }
    private static FerruleXmlDocumentOutputsExecutionException CaptureDynamicMixedOutput(Action action)
    {
        try { action(); }
        catch (FerruleXmlDocumentOutputsExecutionException error) { return error; }
        throw new InvalidOperationException("Expected mixed output failure.");
    }
}
