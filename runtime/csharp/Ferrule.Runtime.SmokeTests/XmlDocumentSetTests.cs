using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void XmlDocumentMembersAndBudgets()
    {
        var writer = new InvalidOperationException("original writer");
        var boundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Output, writer.Message, writer);
        var member = FerruleXmlDocumentExecutionException.Serialization(1, "same.xml", boundary);
        Equal(true, ReferenceEquals(boundary, member.Boundary));
        Equal(true, ReferenceEquals(boundary, member.InnerException));
        Equal(true, ReferenceEquals(writer, member.Boundary.InnerException));
        Equal<FerruleXmlDocumentOutputOwner?>(new FerruleXmlDocumentOutputOwner(FerruleXmlOutputTarget.Primary, 1, "same.xml"), member.Member);

        var schema = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Schema, "descriptor setup", writer);
        var unowned = FerruleXmlDocumentExecutionException.Serialization(4, "would-be.xml", schema);
        Equal<FerruleXmlDocumentOutputOwner?>(null, unowned.Member);
        Equal(true, ReferenceEquals(schema, unowned.InnerException));
        var runtime = new FerruleRuntimeException(FerruleRuntimeError.EmptyDynamicTargetPath, "path", node: 7);
        var mapping = FerruleXmlDocumentExecutionException.FromBoundary(new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Mapping, runtime.Message, runtime));
        Equal<FerruleXmlDocumentOutputOwner?>(null, mapping.Member);
        Equal(true, ReferenceEquals(runtime, mapping.Boundary.InnerException));
        var alignment = FerruleXmlDocumentExecutionException.Alignment("not a document set");
        Equal<FerruleXmlDocumentOutputOwner?>(null, alignment.Member);
        Equal(FerruleXmlBoundaryErrorKind.Output, alignment.Boundary.Kind);

        _ = new FerruleXmlDocumentSetBudget(0);
        _ = new FerruleXmlDocumentSetBudget(4096);
        var count = CaptureDocumentError(() => _ = new FerruleXmlDocumentSetBudget(4097));
        Equal<FerruleXmlDocumentOutputOwner?>(null, count.Member);
        Equal(FerruleXmlBoundaryErrorKind.Output, count.Boundary.Kind);
        var countCause = (FerruleXmlOutputSetResourceException)count.Boundary.InnerException!;
        Equal("xml_output_artifact_count", countCause.Resource);
        Equal(4097UL, countCause.ObservedCount);
        Equal(4096UL, countCause.Limit);

        var budget = new FerruleXmlDocumentSetBudget(5);
        var paths = new[] { "same.xml", "same.xml", "/opaque.xml", "../opaque.xml" };
        for (var index = 0; index < paths.Length; ++index) budget.Charge(index, paths[index], 64 * 1024 * 1024);
        var crossing = CaptureDocumentError(() => budget.Charge(4, "../opaque.xml", 1));
        Equal<FerruleXmlDocumentOutputOwner?>(new FerruleXmlDocumentOutputOwner(FerruleXmlOutputTarget.Primary, 4, "../opaque.xml"), crossing.Member);
        Equal<long?>(null, crossing.Boundary.Bytes);
        Equal<long?>(null, crossing.Boundary.Limit);
        var bytesCause = (FerruleXmlOutputSetResourceException)crossing.Boundary.InnerException!;
        Equal("xml_output_set_utf8_bytes", bytesCause.Resource);
        Equal(256UL * 1024 * 1024 + 1, bytesCause.ObservedCount);
        Equal(256UL * 1024 * 1024, bytesCause.Limit);

        var document = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.DocumentLimit, "document", bytes: 64 * 1024 * 1024 + 1, limit: 64 * 1024 * 1024);
        var limit = FerruleXmlDocumentExecutionException.Serialization(2, "last.xml", document);
        Equal(2, limit.Member!.Index);
        Equal<long?>(64 * 1024 * 1024 + 1, limit.Boundary.Bytes);
        Equal<long?>(64 * 1024 * 1024, limit.Boundary.Limit);
        Equal(true, ReferenceEquals(document, limit.Boundary));
    }

    private static FerruleXmlDocumentExecutionException CaptureDocumentError(Action action)
    {
        try { action(); }
        catch (FerruleXmlDocumentExecutionException error) { return error; }
        throw new InvalidOperationException("Expected XML document execution failure.");
    }
}
