using Ferrule.Runtime;
namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void XmlInputDocumentOwners()
    {
        var decoder = new System.Text.DecoderFallbackException("original decoder");
        var inputBoundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Utf8, decoder.Message, decoder);
        var inputSource = FerruleXmlInputSource.Named(1, "beta");
        var input = FerruleXmlInputDocumentExecutionException.FromInputBoundary(inputSource, inputBoundary);
        Equal<FerruleXmlInputDocumentOwner?>(new FerruleXmlInputDocumentOwner.Input(inputSource), input.Owner);
        Equal(true, ReferenceEquals(inputBoundary, input.Boundary));
        Equal(true, ReferenceEquals(inputBoundary, input.InnerException));
        Equal(true, ReferenceEquals(decoder, input.Boundary.InnerException));

        var schemaBoundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Schema, "descriptor setup", decoder);
        var inputSchema = FerruleXmlInputDocumentExecutionException.FromInputBoundary(FerruleXmlInputSource.Primary, schemaBoundary);
        Equal<FerruleXmlInputDocumentOwner?>(new FerruleXmlInputDocumentOwner.Input(FerruleXmlInputSource.Primary), inputSchema.Owner);
        var outputSchema = FerruleXmlInputDocumentExecutionException.FromDocuments(
            FerruleXmlDocumentExecutionException.Serialization(3, "same.xml", schemaBoundary));
        Equal<FerruleXmlInputDocumentOwner?>(null, outputSchema.Owner);
        Equal(true, ReferenceEquals(schemaBoundary, outputSchema.Boundary));

        var writer = new InvalidOperationException("original writer");
        var outputBoundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Output, writer.Message, writer);
        var originalMember = FerruleXmlDocumentExecutionException.Serialization(1, "../雪😀.xml", outputBoundary);
        var member = FerruleXmlInputDocumentExecutionException.FromDocuments(originalMember);
        Equal<FerruleXmlInputDocumentOwner?>(new FerruleXmlInputDocumentOwner.Member(originalMember.Member!), member.Owner);
        Equal(true, ReferenceEquals(originalMember.Member, ((FerruleXmlInputDocumentOwner.Member)member.Owner!).Output));
        Equal(true, ReferenceEquals(outputBoundary, member.InnerException));
        Equal(true, ReferenceEquals(writer, member.Boundary.InnerException));

        var inputCountOriginal = CaptureStaticInputDocumentAdmission(() => _ = new FerruleXmlInputSetBudget(4097));
        var inputCount = FerruleXmlInputDocumentExecutionException.FromInputBoundary(inputCountOriginal.Input, inputCountOriginal.Boundary);
        Equal<FerruleXmlInputDocumentOwner?>(null, inputCount.Owner);
        Equal(FerruleXmlBoundaryErrorKind.Input, inputCount.Boundary.Kind);
        var inputCountCause = (FerruleXmlInputSetResourceException)inputCount.Boundary.InnerException!;
        Equal("xml_input_artifact_count", inputCountCause.Resource);
        Equal(4097UL, inputCountCause.ObservedCount);
        Equal(4096UL, inputCountCause.Limit);

        var namedOriginal = CaptureStaticInputDocumentAdmission(() => FerruleXmlInputSetBudget.Indices(new[] { "A", "B" }, new[] { "B" }));
        var named = FerruleXmlInputDocumentExecutionException.FromInputBoundary(namedOriginal.Input, namedOriginal.Boundary);
        Equal<FerruleXmlInputDocumentOwner?>(null, named.Owner);
        Equal(FerruleXmlBoundaryErrorKind.Mapping, named.Boundary.Kind);
        Equal(true, ReferenceEquals(namedOriginal.Boundary.InnerException, named.Boundary.InnerException));

        _ = new FerruleXmlDocumentSetBudget(0);
        _ = new FerruleXmlDocumentSetBudget(4096);
        var countOriginal = CaptureDocumentError(() => _ = new FerruleXmlDocumentSetBudget(4097));
        var count = FerruleXmlInputDocumentExecutionException.FromDocuments(countOriginal);
        Equal<FerruleXmlInputDocumentOwner?>(null, count.Owner);
        var countCause = (FerruleXmlOutputSetResourceException)count.Boundary.InnerException!;
        Equal("xml_output_artifact_count", countCause.Resource);
        Equal(4097UL, countCause.ObservedCount);
        Equal(4096UL, countCause.Limit);

        // Counter-only transition; no actual 256 MiB XML is materialized.
        var budget = new FerruleXmlDocumentSetBudget(5);
        for (var index = 0; index < 4; index++) budget.Charge(index, "same.xml", 64 * 1024 * 1024);
        var crossingOriginal = CaptureDocumentError(() => budget.Charge(4, "/opaque.xml", 1));
        var crossing = FerruleXmlInputDocumentExecutionException.FromDocuments(crossingOriginal);
        var crossingOwner = ((FerruleXmlInputDocumentOwner.Member)crossing.Owner!).Output;
        Equal(FerruleXmlOutputTarget.Primary, crossingOwner.Target);
        Equal(4, crossingOwner.Index);
        Equal("/opaque.xml", crossingOwner.Path);
        Equal(true, ReferenceEquals(crossingOriginal.Boundary, crossing.Boundary));
        var crossingCause = (FerruleXmlOutputSetResourceException)crossing.Boundary.InnerException!;
        Equal("xml_output_set_utf8_bytes", crossingCause.Resource);
        Equal(256UL * 1024 * 1024 + 1, crossingCause.ObservedCount);

        var mappingCause = new FerruleRuntimeException(FerruleRuntimeError.EmptyDynamicTargetPath, "path", node: 5);
        var mappingBoundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Mapping, mappingCause.Message, mappingCause);
        var mapping = FerruleXmlInputDocumentExecutionException.FromBoundary(mappingBoundary);
        Equal<FerruleXmlInputDocumentOwner?>(null, mapping.Owner);
        Equal(true, ReferenceEquals(mappingCause, mapping.Boundary.InnerException));
        var alignment = FerruleXmlInputDocumentExecutionException.FromDocuments(FerruleXmlDocumentExecutionException.Alignment("not a document set"));
        Equal<FerruleXmlInputDocumentOwner?>(null, alignment.Owner);
        Equal(FerruleXmlBoundaryErrorKind.Output, alignment.Boundary.Kind);
    }

    private static FerruleXmlExecutionException CaptureStaticInputDocumentAdmission(Action action)
    {
        try { action(); }
        catch (FerruleXmlExecutionException error) { return error; }
        throw new InvalidOperationException("Expected static input admission failure.");
    }
}
