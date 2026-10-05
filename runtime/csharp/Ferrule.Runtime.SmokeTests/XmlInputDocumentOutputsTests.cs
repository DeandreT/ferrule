using Ferrule.Runtime;
namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void XmlInputDocumentOutputsOwners()
    {
        var decoder = new System.Text.DecoderFallbackException("original decoder");
        var boundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Utf8, decoder.Message, decoder);
        var source = FerruleXmlInputSource.Named(2, "unused-padding");
        var input = FerruleXmlInputDocumentOutputsExecutionException.FromInputBoundary(source, boundary);
        Equal<FerruleXmlInputDocumentOutputsOwner?>(new FerruleXmlInputDocumentOutputsOwner.Input(source), input.Owner);
        Equal(true, ReferenceEquals(boundary, input.Boundary));
        Equal(true, ReferenceEquals(boundary, input.InnerException));
        Equal(true, ReferenceEquals(decoder, input.Boundary.InnerException));

        var schema = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Schema, "descriptor", decoder);
        var inputSchema = FerruleXmlInputDocumentOutputsExecutionException.FromInputBoundary(FerruleXmlInputSource.Primary, schema);
        Equal<FerruleXmlInputDocumentOutputsOwner?>(new FerruleXmlInputDocumentOutputsOwner.Input(FerruleXmlInputSource.Primary), inputSchema.Owner);
        foreach (var original in new[] {
            FerruleXmlDocumentOutputsExecutionException.PrimarySerialization(schema),
            FerruleXmlDocumentOutputsExecutionException.NamedSerialization(1, "a-beta", 3, "same.xml", schema),
        })
        {
            var error = FerruleXmlInputDocumentOutputsExecutionException.FromOutputs(original);
            Equal<FerruleXmlInputDocumentOutputsOwner?>(null, error.Owner);
            Equal(true, ReferenceEquals(schema, error.Boundary));
            Equal(true, ReferenceEquals(schema, error.InnerException));
            Equal(true, ReferenceEquals(decoder, error.Boundary.InnerException));
        }

        var writer = new InvalidOperationException("original writer");
        var outputBoundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Output, writer.Message, writer);
        foreach (var original in new[] {
            FerruleXmlDocumentOutputsExecutionException.PrimarySerialization(outputBoundary),
            FerruleXmlDocumentOutputsExecutionException.NamedSerialization(1, "a-beta", 1, "../雪😀.xml", outputBoundary),
        })
        {
            var error = FerruleXmlInputDocumentOutputsExecutionException.FromOutputs(original);
            Equal<FerruleXmlInputDocumentOutputsOwner?>(new FerruleXmlInputDocumentOutputsOwner.Output(original.Owner!), error.Owner);
            Equal(true, ReferenceEquals(original.Owner, ((FerruleXmlInputDocumentOutputsOwner.Output)error.Owner!).Owner));
            Equal(true, ReferenceEquals(outputBoundary, error.Boundary));
            Equal(true, ReferenceEquals(outputBoundary, error.InnerException));
            Equal(true, ReferenceEquals(writer, error.Boundary.InnerException));
        }

        var originalInputCount = CaptureMixedInputAdmission(() => _ = new FerruleXmlInputSetBudget(4097));
        var inputCount = FerruleXmlInputDocumentOutputsExecutionException.FromInputBoundary(originalInputCount.Input, originalInputCount.Boundary);
        Equal<FerruleXmlInputDocumentOutputsOwner?>(null, inputCount.Owner);
        Equal(true, ReferenceEquals(originalInputCount.Boundary, inputCount.Boundary));
        var inputCountCause = (FerruleXmlInputSetResourceException)inputCount.Boundary.InnerException!;
        Equal("xml_input_artifact_count", inputCountCause.Resource);
        Equal(4097UL, inputCountCause.ObservedCount);
        Equal(4096UL, inputCountCause.Limit);
        var originalNames = CaptureMixedInputAdmission(() => FerruleXmlInputSetBudget.Indices(new[] { "z-rates", "a-count" }, new[] { "a-count" }));
        var names = FerruleXmlInputDocumentOutputsExecutionException.FromInputBoundary(originalNames.Input, originalNames.Boundary);
        Equal<FerruleXmlInputDocumentOutputsOwner?>(null, names.Owner);
        Equal(FerruleXmlBoundaryErrorKind.Mapping, names.Boundary.Kind);
        Equal(true, ReferenceEquals(originalNames.Boundary.InnerException, names.Boundary.InnerException));

        var originalCount = CaptureMixedOutputs(() => _ = new FerruleXmlDocumentOutputsBudget(4097));
        var count = FerruleXmlInputDocumentOutputsExecutionException.FromOutputs(originalCount);
        Equal<FerruleXmlInputDocumentOutputsOwner?>(null, count.Owner);
        var countCause = (FerruleXmlOutputSetResourceException)count.Boundary.InnerException!;
        Equal("xml_output_artifact_count", countCause.Resource);
        Equal(4097UL, countCause.ObservedCount);
        Equal(4096UL, countCause.Limit);
        var alignment = FerruleXmlInputDocumentOutputsExecutionException.FromOutputs(FerruleXmlDocumentOutputsExecutionException.Alignment("incomplete envelopes"));
        Equal<FerruleXmlInputDocumentOutputsOwner?>(null, alignment.Owner);
        Equal(FerruleXmlBoundaryErrorKind.Output, alignment.Boundary.Kind);
        var runtime = new FerruleRuntimeException(FerruleRuntimeError.EmptyDynamicTargetPath, "path", node: 3);
        var mappingBoundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Mapping, runtime.Message, runtime);
        var mapping = FerruleXmlInputDocumentOutputsExecutionException.FromBoundary(mappingBoundary);
        Equal<FerruleXmlInputDocumentOutputsOwner?>(null, mapping.Owner);
        Equal(true, ReferenceEquals(mappingBoundary, mapping.InnerException));
        Equal(true, ReferenceEquals(runtime, mapping.Boundary.InnerException));

        // Independent counter-only ledgers, not actual large serialized documents.
        var inputBudget = new FerruleXmlInputSetBudget(5);
        foreach (var owner in new[] { FerruleXmlInputSource.Primary,
            FerruleXmlInputSource.Named(0, "z-rates"), FerruleXmlInputSource.Named(1, "a-count"), FerruleXmlInputSource.Named(2, "unused-padding") })
            inputBudget.Charge(owner, 64L * 1024 * 1024);
        var originalInput = CaptureMixedInputAdmission(() => inputBudget.Charge(FerruleXmlInputSource.Named(3, "tail"), 1));
        var crossingInput = FerruleXmlInputDocumentOutputsExecutionException.FromInputBoundary(originalInput.Input, originalInput.Boundary);
        Equal<FerruleXmlInputDocumentOutputsOwner?>(new FerruleXmlInputDocumentOutputsOwner.Input(FerruleXmlInputSource.Named(3, "tail")), crossingInput.Owner);
        Equal(true, ReferenceEquals(originalInput.Boundary, crossingInput.Boundary));
        var inputBytes = (FerruleXmlInputSetResourceException)crossingInput.Boundary.InnerException!;
        Equal("xml_input_set_utf8_bytes", inputBytes.Resource);
        Equal(256UL * 1024 * 1024 + 1, inputBytes.ObservedCount);
        Equal(256UL * 1024 * 1024, inputBytes.Limit);
        var budget = new FerruleXmlDocumentOutputsBudget(5);
        budget.ChargePrimary(64L * 1024 * 1024);
        budget.ChargeNamed(0, "z-alpha", 0, "same.xml", 64L * 1024 * 1024);
        budget.ChargeNamed(1, "a-beta", 0, "same.xml", 64L * 1024 * 1024);
        budget.ChargeNamed(1, "a-beta", 1, "/opaque.xml", 64L * 1024 * 1024);
        var originalCrossing = CaptureMixedOutputs(() => budget.ChargeNamed(1, "a-beta", 2, "../last.xml", 1));
        var crossing = FerruleXmlInputDocumentOutputsExecutionException.FromOutputs(originalCrossing);
        Equal<FerruleXmlInputDocumentOutputsOwner?>(new FerruleXmlInputDocumentOutputsOwner.Output(
            new FerruleXmlDocumentOutputsOwner.NamedMember(1, "a-beta", 2, "../last.xml")), crossing.Owner);
        Equal(true, ReferenceEquals(originalCrossing.Owner, ((FerruleXmlInputDocumentOutputsOwner.Output)crossing.Owner!).Owner));
        Equal(true, ReferenceEquals(originalCrossing.Boundary, crossing.Boundary));
        var outputBytes = (FerruleXmlOutputSetResourceException)crossing.Boundary.InnerException!;
        Equal("xml_output_set_utf8_bytes", outputBytes.Resource);
        Equal(256UL * 1024 * 1024 + 1, outputBytes.ObservedCount);
        Equal(256UL * 1024 * 1024, outputBytes.Limit);
    }

    private static FerruleXmlExecutionException CaptureMixedInputAdmission(Action action)
    {
        try { action(); }
        catch (FerruleXmlExecutionException error) { return error; }
        throw new InvalidOperationException("Expected trusted mixed-input admission failure.");
    }
    private static FerruleXmlDocumentOutputsExecutionException CaptureMixedOutputs(Action action)
    {
        try { action(); }
        catch (FerruleXmlDocumentOutputsExecutionException error) { return error; }
        throw new InvalidOperationException("Expected mixed-output failure.");
    }
}
