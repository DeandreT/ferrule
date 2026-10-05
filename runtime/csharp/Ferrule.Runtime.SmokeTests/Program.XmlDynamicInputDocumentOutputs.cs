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
