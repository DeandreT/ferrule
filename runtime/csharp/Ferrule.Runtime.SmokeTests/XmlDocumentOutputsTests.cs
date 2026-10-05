using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void XmlDocumentOutputsOwnersAndBudgets()
    {
        var writer = new InvalidOperationException("original writer");
        var boundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Output, writer.Message, writer);
        var primary = FerruleXmlDocumentOutputsExecutionException.PrimarySerialization(boundary);
        Equal<FerruleXmlDocumentOutputsOwner?>(new FerruleXmlDocumentOutputsOwner.Primary(), primary.Owner);
        foreach (var field in new[] { "DeclarationIndex", "Name", "Index", "Path" })
            Equal(true, typeof(FerruleXmlDocumentOutputsOwner.Primary).GetProperty(field) is null);
        var named = FerruleXmlDocumentOutputsExecutionException.NamedSerialization(3, "audit", 1, "../same.xml", boundary);
        Equal<FerruleXmlDocumentOutputsOwner?>(new FerruleXmlDocumentOutputsOwner.NamedMember(3, "audit", 1, "../same.xml"), named.Owner);
        foreach (var error in new[] { primary, named })
        {
            Equal(true, ReferenceEquals(boundary, error.Boundary));
            Equal(true, ReferenceEquals(boundary, error.InnerException));
            Equal(true, ReferenceEquals(writer, error.Boundary.InnerException));
        }
        var schema = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Schema, "descriptor setup", writer);
        foreach (var unowned in new[] {
            FerruleXmlDocumentOutputsExecutionException.PrimarySerialization(schema),
            FerruleXmlDocumentOutputsExecutionException.NamedSerialization(0, "audit", 1, "/opaque.xml", schema) })
        {
            Equal<FerruleXmlDocumentOutputsOwner?>(null, unowned.Owner);
            Equal(true, ReferenceEquals(schema, unowned.InnerException));
        }
        var runtime = new FerruleRuntimeException(FerruleRuntimeError.EmptyDynamicTargetPath, "path", node: 7);
        var mapping = FerruleXmlDocumentOutputsExecutionException.FromBoundary(new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Mapping, runtime.Message, runtime));
        Equal<FerruleXmlDocumentOutputsOwner?>(null, mapping.Owner);
        Equal(true, ReferenceEquals(runtime, mapping.Boundary.InnerException));
        var input = FerruleXmlDocumentOutputsExecutionException.FromBoundary(new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Input, "parse", writer));
        Equal<FerruleXmlDocumentOutputsOwner?>(null, input.Owner);
        Equal(true, ReferenceEquals(writer, input.Boundary.InnerException));
        var alignment = FerruleXmlDocumentOutputsExecutionException.Alignment("artifact count overflow");
        Equal<FerruleXmlDocumentOutputsOwner?>(null, alignment.Owner);
        Equal(FerruleXmlBoundaryErrorKind.Output, alignment.Boundary.Kind);
        Equal(true, alignment.Boundary.InnerException is InvalidOperationException);
        var zero = CaptureDocumentOutputsError(() => _ = new FerruleXmlDocumentOutputsBudget(0));
        Equal<FerruleXmlDocumentOutputsOwner?>(null, zero.Owner);
        Equal(true, zero.Boundary.InnerException is InvalidOperationException);
        _ = new FerruleXmlDocumentOutputsBudget(1);
        _ = new FerruleXmlDocumentOutputsBudget(1 + 4095);
        var count = CaptureDocumentOutputsError(() => _ = new FerruleXmlDocumentOutputsBudget(1 + 4096));
        Equal<FerruleXmlDocumentOutputsOwner?>(null, count.Owner);
        Equal<long?>(null, count.Boundary.Bytes);
        Equal<long?>(null, count.Boundary.Limit);
        var countCause = (FerruleXmlOutputSetResourceException)count.Boundary.InnerException!;
        Equal("xml_output_artifact_count", countCause.Resource);
        Equal(4097UL, countCause.ObservedCount);
        Equal(4096UL, countCause.Limit);
        var budget = new FerruleXmlDocumentOutputsBudget(5);
        budget.ChargePrimary(64 * 1024 * 1024);
        var paths = new[] { "same.xml", "same.xml", "/opaque.xml" };
        for (var index = 0; index < paths.Length; ++index) budget.ChargeNamed(0, "audit", index, paths[index], 64 * 1024 * 1024);
        var crossing = CaptureDocumentOutputsError(() => budget.ChargeNamed(0, "audit", 3, "../opaque.xml", 1));
        Equal<FerruleXmlDocumentOutputsOwner?>(new FerruleXmlDocumentOutputsOwner.NamedMember(0, "audit", 3, "../opaque.xml"), crossing.Owner);
        Equal<long?>(null, crossing.Boundary.Bytes);
        Equal<long?>(null, crossing.Boundary.Limit);
        var bytesCause = (FerruleXmlOutputSetResourceException)crossing.Boundary.InnerException!;
        Equal("xml_output_set_utf8_bytes", bytesCause.Resource);
        Equal(256UL * 1024 * 1024 + 1, bytesCause.ObservedCount);
        Equal(256UL * 1024 * 1024, bytesCause.Limit);
        var document = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.DocumentLimit, "document", bytes: 64 * 1024 * 1024 + 1, limit: 64 * 1024 * 1024);
        var documentPrimary = FerruleXmlDocumentOutputsExecutionException.PrimarySerialization(document);
        var documentNamed = FerruleXmlDocumentOutputsExecutionException.NamedSerialization(0, "audit", 1, "same.xml", document);
        Equal<FerruleXmlDocumentOutputsOwner?>(new FerruleXmlDocumentOutputsOwner.Primary(), documentPrimary.Owner);
        Equal<FerruleXmlDocumentOutputsOwner?>(new FerruleXmlDocumentOutputsOwner.NamedMember(0, "audit", 1, "same.xml"), documentNamed.Owner);
        foreach (var limit in new[] { documentPrimary, documentNamed })
        {
            Equal<long?>(64 * 1024 * 1024 + 1, limit.Boundary.Bytes);
            Equal<long?>(64 * 1024 * 1024, limit.Boundary.Limit);
            Equal(true, ReferenceEquals(document, limit.Boundary));
            Equal<Exception?>(null, limit.Boundary.InnerException);
        }
    }

    private static FerruleXmlDocumentOutputsExecutionException CaptureDocumentOutputsError(Action action)
    {
        try { action(); }
        catch (FerruleXmlDocumentOutputsExecutionException error) { return error; }
        throw new InvalidOperationException("Expected mixed XML document output failure.");
    }
}
