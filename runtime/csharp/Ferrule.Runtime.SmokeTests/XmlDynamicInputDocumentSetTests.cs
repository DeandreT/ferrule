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
        XmlDynamicPrimaryMultipleSourceChannels();
    }

    private const string DynamicPrimaryFloatSchema = """{"name":"Catalog","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[{"name":"Amount","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"float"}}]}}""";
    private const string DynamicPrimaryIntSchema = """{"name":"Codes","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"group","children":[{"name":"Code","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"int"}}]}}""";
    private static FerruleXmlDynamicSourcePolicy[] DynamicPrimaryPolicies() => new[] {
        new FerruleXmlDynamicSourcePolicy(1, "catalog", DynamicPrimaryFloatSchema),
        new FerruleXmlDynamicSourcePolicy(3, "codes", DynamicPrimaryIntSchema),
    };

    private static void XmlDynamicPrimaryMultipleSourceChannels()
    {
        var host = new DynamicPrimaryMultipleBytes(false);
        var successful = FerruleXmlDynamicSourceAdapter.ForSources(host, DynamicPrimaryPolicies(), new FerruleXmlInputSetBudget(3));
        var catalog = (FerruleGroup)successful.Load("catalog", "same.xml");
        var codes = (FerruleGroup)successful.Load("codes", "same.xml");
        Equal(true, catalog.TryGetField("Amount", out var amount));
        Equal(true, codes.TryGetField("Code", out var code));
        Equal(FerruleValueKind.Double, ((FerruleScalar)amount!).Value.Kind);
        Equal(FerruleValueKind.Int64, ((FerruleScalar)code!).Value.Kind);
        Equal(BitConverter.DoubleToInt64Bits(-0.0), BitConverter.DoubleToInt64Bits(((FerruleScalar)amount!).Value.DoubleValue));
        Equal(9_007_199_254_740_993L, ((FerruleScalar)code!).Value.Int64Value);
        Equal(2, host.Calls.Count);
        Equal(("catalog", "same.xml"), host.Calls[0]);
        Equal(("codes", "same.xml"), host.Calls[1]);

        // Seeded counters and tiny buffers test wrapper ownership, not genuine resource caps.
        for (var mode = 0; mode < 3; mode++)
        {
            var failedHost = new DynamicPrimaryMultipleBytes(mode == 0);
            var budget = new FerruleXmlInputSetBudget(mode == 1 ? 4095 : 3);
            if (mode == 2)
                budget.Charge(FerruleXmlInputSource.Primary, 256L * 1024 * 1024 - DynamicPrimaryMultipleBytes.Catalog.Length);
            var adapter = FerruleXmlDynamicSourceAdapter.ForSources(failedHost, DynamicPrimaryPolicies(), budget);
            _ = adapter.Load("catalog", "same.xml");
            var marker = CaptureDynamicPrimarySecondLoad(adapter);
            var runtime = new FerruleRuntimeException(FerruleRuntimeError.DynamicSourceLoad, "original second-source load", marker, detail: "same.xml", sourceField: "codes");
            var original = adapter.Recover(runtime);
            var request = original.Request!;
            var error = FerruleXmlDynamicInputDocumentExecutionException.FromDynamicInputBoundary(request, original.Boundary);
            Equal<FerruleXmlDynamicInputDocumentOwner?>(new FerruleXmlDynamicInputDocumentOwner.Input(FerruleXmlInputSource.Named(3, "codes")), error.Owner);
            Equal(3, request.DeclarationIndex);
            Equal("codes", request.Source);
            Equal("same.xml", request.Path);
            Equal(2UL, request.Ordinal);
            Equal(mode != 1, request.CallbackInvoked);
            Equal(true, ReferenceEquals(request, error.Request));
            Equal(true, ReferenceEquals(original.Boundary, error.Boundary));
            Equal(true, ReferenceEquals(original.Boundary, error.InnerException));
            Equal(true, ReferenceEquals(original.Boundary.InnerException, error.Boundary.InnerException));
            Equal(mode == 1 ? 1 : 2, failedHost.Calls.Count);
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
                Equal(mode == 1 ? 4097UL : 256UL * 1024 * 1024 + (ulong)DynamicPrimaryMultipleBytes.Codes.Length, resource.ObservedCount);
                Equal(mode == 1 ? 4096UL : 256UL * 1024 * 1024, resource.Limit);
            }
            // This adapter is terminal after the failed load; every mode uses a fresh one.
        }
    }

    private sealed class DynamicPrimaryMultipleBytes(bool invalidCodes) : IFerruleDynamicXmlSourceLoader
    {
        public static readonly byte[] Catalog = System.Text.Encoding.UTF8.GetBytes("<Catalog><Amount>-0.0</Amount></Catalog>");
        public static readonly byte[] Codes = System.Text.Encoding.UTF8.GetBytes("<Codes><Code>9007199254740993</Code></Codes>");
        public readonly List<(string Source, string Path)> Calls = new();
        public byte[] Load(string sourceName, string logicalPath)
        {
            Calls.Add((sourceName, logicalPath));
            return sourceName == "catalog" ? (byte[])Catalog.Clone() :
                invalidCodes ? new byte[] { 0xff } : (byte[])Codes.Clone();
        }
    }
    private static Exception CaptureDynamicPrimarySecondLoad(FerruleXmlDynamicSourceAdapter adapter)
    {
        try { _ = adapter.Load("codes", "same.xml"); }
        catch (Exception error) { return error; }
        throw new InvalidOperationException("Expected one terminal second-source refusal.");
    }

}
