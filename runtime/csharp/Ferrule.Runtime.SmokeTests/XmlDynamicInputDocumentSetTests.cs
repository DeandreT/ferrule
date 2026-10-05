using Ferrule.Runtime;
namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void XmlDynamicInputDocumentOwners()
    {
        var decoder = new System.Text.DecoderFallbackException("original decoder");
        var boundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Utf8, decoder.Message, decoder);
        var request = new FerruleXmlDynamicInputRequest(1, "catalog", "b.xml", 2UL, true);
        var dynamicInput = FerruleXmlDynamicInputDocumentExecutionException.FromDynamicInputBoundary(request, boundary);
        Equal<FerruleXmlDynamicInputDocumentOwner?>(new FerruleXmlDynamicInputDocumentOwner.Input(FerruleXmlInputSource.Named(1, "catalog")), dynamicInput.Owner);
        Equal(true, ReferenceEquals(request, dynamicInput.Request));
        Equal(true, ReferenceEquals(boundary, dynamicInput.Boundary));
        Equal(true, ReferenceEquals(boundary, dynamicInput.InnerException));
        Equal(true, ReferenceEquals(decoder, dynamicInput.Boundary.InnerException));

        var schema = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Schema, "original descriptor", decoder);
        var input = FerruleXmlDynamicInputDocumentExecutionException.FromInputBoundary(FerruleXmlInputSource.Named(2, "labels"), schema);
        Equal<FerruleXmlDynamicInputDocumentOwner?>(new FerruleXmlDynamicInputDocumentOwner.Input(FerruleXmlInputSource.Named(2, "labels")), input.Owner);
        Equal<FerruleXmlDynamicInputRequest?>(null, input.Request);
        Equal(true, ReferenceEquals(schema, input.Boundary));
        var schemaOutput = FerruleXmlDynamicInputDocumentExecutionException.FromDocuments(FerruleXmlDocumentExecutionException.Serialization(1, "same.xml", schema));
        Equal<FerruleXmlDynamicInputDocumentOwner?>(null, schemaOutput.Owner);
        Equal<FerruleXmlDynamicInputRequest?>(null, schemaOutput.Request);
        Equal(true, ReferenceEquals(schema, schemaOutput.Boundary));

        var writer = new InvalidOperationException("original writer");
        var outputBoundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Output, writer.Message, writer);
        var original = FerruleXmlDocumentExecutionException.Serialization(1, "../雪😀.xml", outputBoundary);
        var output = FerruleXmlDynamicInputDocumentExecutionException.FromDocuments(original);
        Equal<FerruleXmlDynamicInputDocumentOwner?>(new FerruleXmlDynamicInputDocumentOwner.Member(original.Member!), output.Owner);
        Equal(true, ReferenceEquals(original.Member, ((FerruleXmlDynamicInputDocumentOwner.Member)output.Owner!).Output));
        Equal(true, ReferenceEquals(outputBoundary, output.InnerException));
        Equal(true, ReferenceEquals(writer, output.Boundary.InnerException));
        Equal<FerruleXmlDynamicInputRequest?>(null, output.Request);

        var host = new FerruleRuntimeException(FerruleRuntimeError.EmptyDynamicTargetPath, "original mapping", node: 0);
        var mapping = FerruleXmlDynamicInputDocumentExecutionException.FromBoundary(new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Mapping, host.Message, host));
        Equal<FerruleXmlDynamicInputDocumentOwner?>(null, mapping.Owner);
        Equal<FerruleXmlDynamicInputRequest?>(null, mapping.Request);
        Equal(true, ReferenceEquals(host, mapping.Boundary.InnerException));
        var alignment = FerruleXmlDynamicInputDocumentExecutionException.FromDocuments(FerruleXmlDocumentExecutionException.Alignment("not a document set"));
        Equal<FerruleXmlDynamicInputDocumentOwner?>(null, alignment.Owner);
        Equal<FerruleXmlDynamicInputRequest?>(null, alignment.Request);
    }
}
