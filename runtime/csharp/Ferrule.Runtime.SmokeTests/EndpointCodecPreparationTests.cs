using System.Text;
using System.Text.Json.Nodes;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    /// <summary>The JSON5 order fixture of crates/codegen-runtime/tests/endpoint_codec_preparation.rs.</summary>
    private static string EndpointJson5Schema(string name)
    {
        static string Scalar(string field, string type) =>
            $"{{\"name\":\"{field}\",\"kind\":{{\"kind\":\"scalar\",\"ty\":\"{type}\"}}}}";
        static string Group(string field, params string[] children) =>
            $"{{\"name\":\"{field}\",\"kind\":{{\"kind\":\"group\",\"children\":[{string.Join(",", children)}]}}}}";
        var children = new List<string>
        {
            Scalar("Id", "string"), Scalar("Quantity", "int"), Scalar("Total", "float"),
            Scalar("Paid", "bool"), Scalar("Note", "string"),
            Group("Customer", Scalar("Name", "string"), Scalar("City", "string")),
            Group("Shipping", Scalar("Carrier", "string"), Scalar("Days", "int")),
        };
        for (var index = 0; index < 43; index++) children.Add(Scalar($"Attribute{index:00}", "string"));
        return Group(name, [.. children]);
    }

    private static readonly string EndpointJson5Source = EndpointJson5Schema("Source");
    private static readonly string EndpointJson5Target = EndpointJson5Schema("Target");

    private static string EndpointJson5Document(int noteLength) =>
        "// order\n{\n  Id: 'ORD-000042',\n  Quantity: 0x10,\n  Total: +1234.5,\n  Paid: true,\n  Note: '" +
        new string('n', noteLength) +
        "',\n  Customer: { Name: 'Example Customer', City: \"Example City\", },\n  Shipping: { Carrier: 'Post', Days: 3 },\n  Attribute00: 'alpha', Attribute01: 'beta',\n}\n";

    /// <summary>The pattern-free order fixture used by the XML boundaries.</summary>
    private static readonly string EndpointXmlSchema = EndpointXmlSchemaText();

    private static string EndpointXmlSchemaText()
    {
        static string Plain(string name, string type) =>
            $"{{\"name\":\"{name}\",\"kind\":{{\"kind\":\"scalar\",\"ty\":\"{type}\"}}}}";
        var children = new List<string>
        {
            Plain("Id", "string"), Plain("Customer", "string"), Plain("Currency", "string"), Plain("Total", "float"),
            "{\"name\":\"Line\",\"repeating\":true,\"kind\":{\"kind\":\"group\",\"children\":[" +
            Plain("Sku", "string") + "," + Plain("Quantity", "int") + "," + Plain("Price", "float") + "," +
            Plain("Note", "string") + "]}}",
        };
        for (var index = 0; index < 48; index++) children.Add(Plain($"Attribute{index:00}", "string"));
        return "{\"name\":\"Order\",\"kind\":{\"kind\":\"group\",\"children\":[" + string.Join(",", children) + "]}}";
    }

    /// <summary>A flat root with two <c>xsi:type</c> alternatives, the observed primary XML profile.</summary>
    private const string EndpointObservedXmlSchema =
        "{\"name\":\"Envelope\",\"xml_namespace\":{\"kind\":\"unqualified\"},\"xml_type_alternatives\":true,\"xml_default_type\":\"Base\"," +
        "\"kind\":{\"kind\":\"group\",\"children\":[" +
        "{\"name\":\"Token\",\"xml_namespace\":{\"kind\":\"unqualified\"},\"attribute\":true,\"xml_attribute_required\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}," +
        "{\"name\":\"Supplement\",\"xml_namespace\":{\"kind\":\"unqualified\"},\"attribute\":true,\"xml_attribute_required\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}" +
        "],\"alternatives\":[{\"name\":\"Base\",\"members\":[\"Token\"]},{\"name\":\"Extended\",\"members\":[\"Token\",\"Supplement\"]}]}}";

    private static FerruleInstance EndpointXmlInstance(int lines)
    {
        var items = Enumerable.Range(0, lines).Select(index => (FerruleInstance)Group(
            Field("Sku", Scalar(Text($"AB-{index % 10_000:0000}"))),
            Field("Quantity", Scalar(FerruleValue.FromInt64(index % 999 + 1))),
            Field("Price", Scalar(FerruleValue.FromDouble(1.25 + index % 100))),
            Field("Note", Scalar(Text($"line {index} of the order")))));
        return Group(
            Field("Id", Scalar(Text("ORD-000042"))), Field("Customer", Scalar(Text("Example Customer"))),
            Field("Currency", Scalar(Text("EUR"))), Field("Total", Scalar(FerruleValue.FromDouble(1234.5))),
            Field("Line", new FerruleRepeated(items.ToArray())),
            Field("Attribute00", Scalar(Text("alpha"))), Field("Attribute01", Scalar(Text("beta"))));
    }

    private static readonly string EndpointX12Descriptor = X12Descriptor(X12RootSchema(EndpointX12Notes()));

    private static JsonObject EndpointX12Notes()
    {
        var notes = X12GroupSchema("NTE", X12ScalarSchema("NTE01"), X12ScalarSchema("NTE02"));
        notes["repeating"] = true;
        return notes;
    }

    private static string EndpointX12Document(int notes)
    {
        var text = new StringBuilder(X12Prefix);
        for (var index = 0; index < notes; index++)
            text.Append(System.Globalization.CultureInfo.InvariantCulture, $"NTE*note {index}*free form text of the note~\n");
        return text.Append(System.Globalization.CultureInfo.InvariantCulture, $"SE*{notes + 2}*0001~\nGE*1*1~\nIEA*1*000000001~\n").ToString();
    }

    private static FerruleInstance EndpointJson5Mapping(FerruleInstance input) =>
        Group(((FerruleGroup)input).Fields.Select(field => Field(field.Name, field.Value)).ToArray());

    /// <summary>Every success and failure detail an endpoint exposes, including its causes.</summary>
    private static string EndpointOutcome(Func<object> call)
    {
        try
        {
            return "ok:" + EndpointRender(call());
        }
        catch (Exception error)
        {
            var text = new StringBuilder("error");
            for (Exception? cause = error; cause is not null; cause = cause.InnerException)
            {
                text.Append('|').Append(cause.GetType().FullName).Append(':').Append(cause.Message);
                text.Append(cause switch
                {
                    FerruleJson5BoundaryException json5 => $":{json5.Stage}:{json5.SchemaSide}:{json5.Resource}:{json5.Requested}:{json5.Maximum}",
                    FerruleXmlBoundaryException xml => $":{xml.Kind}:{xml.Detail}:{xml.Bytes}:{xml.Limit}",
                    FerruleX12Exception x12 => $":{x12.Error}:{x12.SegmentIndex}:{x12.FieldPath}",
                    FerruleRuntimeException runtime => $":{runtime.Error}",
                    _ => "",
                });
            }
            return text.ToString();
        }
    }

    private static string EndpointRender(object value) => value switch
    {
        string text => "text:" + text,
        byte[] bytes => "bytes:" + Convert.ToHexString(bytes),
        FerruleValue scalar => "value:" + scalar.Kind + ":" + scalar,
        FerruleXmlPreparedUtf8Document prepared => $"utf8:{prepared.Utf8ByteCount}:" + Convert.ToHexString(prepared.ToByteArray()),
        FerruleInstance instance => "instance:" + EndpointRenderInstance(instance),
        _ => throw new InvalidOperationException("Unexpected endpoint result."),
    };

    private static string EndpointRenderInstance(FerruleInstance instance) => instance switch
    {
        FerruleScalar scalar => $"{scalar.Value.Kind}({scalar.Value})",
        FerruleGroup group => "{" + string.Join(",", group.Fields.Select(field => field.Name + "=" + EndpointRenderInstance(field.Value))) + "}",
        FerruleRepeated repeated => "[" + string.Join(",", repeated.Items.Select(EndpointRenderInstance)) + "]",
        _ => instance.GetType().Name,
    };

    /// <summary>One endpoint call through a descriptor, so a test can pass a shared or a fresh copy.</summary>
    private sealed record EndpointCase(string Name, string Descriptor, Func<string, object> Call);

    private static IEnumerable<EndpointCase> EndpointCases()
    {
        var json5Small = EndpointJson5Document(32);
        var json5Mapping = (Func<FerruleInstance, FerruleInstance>)EndpointJson5Mapping;
        string Json5Target(string source) => ReferenceEquals(source, EndpointJson5Source) ? EndpointJson5Target : UnpreparedCopy(EndpointJson5Target);
        yield return new("JSON5 text", EndpointJson5Source, source => FerruleJson5.Execute(source, Json5Target(source), json5Small, json5Mapping));
        yield return new("JSON5 bytes", EndpointJson5Source, source => FerruleJson5.ExecuteBytes(source, Json5Target(source), Encoding.UTF8.GetBytes(json5Small), json5Mapping));
        // Same descriptor on both sides: a shared source preparation also serves the target.
        yield return new("JSON5 one descriptor", EndpointJson5Source, source => FerruleJson5.Execute(source, source, json5Small, json5Mapping));
        yield return new("JSON5 syntax before mapping", EndpointJson5Source, source => FerruleJson5.Execute(source, Json5Target(source), "{Id: 'x',,}", json5Mapping));
        yield return new("JSON5 input shape", EndpointJson5Source, source => FerruleJson5.Execute(source, Json5Target(source), "{Id: 1}", json5Mapping));
        yield return new("JSON5 unknown field", EndpointJson5Source, source => FerruleJson5.Execute(source, Json5Target(source), "{Missing: 1}", json5Mapping));
        yield return new("JSON5 mapping failure", EndpointJson5Source, source => FerruleJson5.Execute(source, Json5Target(source), json5Small, _ => throw new InvalidOperationException("mapping failed")));
        yield return new("JSON5 output shape", EndpointJson5Source, source => FerruleJson5.Execute(source, Json5Target(source), json5Small, _ => Scalar(Text("not an object"))));
        yield return new("JSON5 encoding before schema", "{", source => FerruleJson5.ExecuteBytes(source, source, [0xFF], json5Mapping));
        yield return new("JSON5 invalid source schema", "{", source => FerruleJson5.Execute(source, EndpointJson5Target, json5Small, json5Mapping));
        yield return new("JSON5 invalid target schema", "{\"name\":\"Root\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
            target => FerruleJson5.Execute(EndpointJson5Source, target, json5Small, json5Mapping));

        var x12Small = EndpointX12Document(2);
        var x12Instance = FerruleX12.ParseEmbedded(EndpointX12Descriptor, x12Small);
        yield return new("X12 parse", EndpointX12Descriptor, descriptor => FerruleX12.ParseEmbedded(descriptor, x12Small));
        yield return new("X12 parse bytes", EndpointX12Descriptor, descriptor => FerruleX12.ParseEmbeddedBytes(descriptor, Encoding.UTF8.GetBytes(x12Small)));
        yield return new("X12 parse syntax", EndpointX12Descriptor, descriptor => FerruleX12.ParseEmbedded(descriptor, "ISA*"));
        yield return new("X12 parse envelope", EndpointX12Descriptor, descriptor => FerruleX12.ParseEmbedded(descriptor, x12Small.Replace("SE*4*", "SE*5*", StringComparison.Ordinal)));
        yield return new("X12 serialize", EndpointX12Descriptor, descriptor => FerruleX12.SerializeEmbedded(descriptor, x12Instance));
        yield return new("X12 serialize bytes", EndpointX12Descriptor, descriptor => FerruleX12.SerializeEmbeddedBytes(descriptor, x12Instance));
        yield return new("X12 serialize invalid", EndpointX12Descriptor, descriptor => FerruleX12.SerializeEmbedded(descriptor, Group(Field("ISA", Group()))));
        yield return new("X12 invalid descriptor", "{\"version\":\"004010\"}", descriptor => FerruleX12.ParseEmbedded(descriptor, x12Small));
        yield return new("X12 input limit before descriptor", "{", descriptor => FerruleX12.ParseEmbeddedBytes(descriptor, new byte[FerruleX12.MaximumDocumentBytes + 1]));

        var xmlInstance = EndpointXmlInstance(3);
        var xmlSmall = FerruleXml.SerializeDocumentEmbedded(EndpointXmlSchema, xmlInstance, true, true, null, null);
        const string hints = "{\"no_namespace_location\":\"order.xsd\"}";
        yield return new("XML document", EndpointXmlSchema, descriptor => FerruleXml.SerializeDocumentEmbedded(descriptor, xmlInstance, true, true, null, null));
        yield return new("XML document namespace", EndpointXmlSchema, descriptor => FerruleXml.SerializeDocumentEmbedded(descriptor, xmlInstance, false, false, "urn:order", hints));
        yield return new("XML document reserved namespace", EndpointXmlSchema, descriptor => FerruleXml.SerializeDocumentEmbedded(descriptor, xmlInstance, false, false, "http://www.w3.org/2000/xmlns/", null));
        yield return new("XML document invalid hints", EndpointXmlSchema, descriptor => FerruleXml.SerializeDocumentEmbedded(descriptor, xmlInstance, false, false, null, "{}"));
        yield return new("XML document invalid output", EndpointXmlSchema, descriptor => FerruleXml.SerializeDocumentEmbedded(descriptor, Group(Field("Id", Scalar(Text("\u0001")))), false, false, null, null));
        yield return new("XML document UTF-8", EndpointXmlSchema, descriptor => FerruleXml.PrepareDocumentEmbeddedUtf8(descriptor, xmlInstance, true, true, null, null));
        yield return new("XML document invalid schema", "{\"name\":\"1\",\"kind\":{\"kind\":\"group\",\"children\":[]}}",
            descriptor => FerruleXml.SerializeDocumentEmbedded(descriptor, xmlInstance, false, false, null, null));
        yield return new("XML node", EndpointXmlSchema, descriptor => FerruleXml.SerializeEmbedded(7, descriptor, xmlInstance, false, true, "urn:order"));
        yield return new("XML node empty namespace", EndpointXmlSchema, descriptor => FerruleXml.SerializeEmbedded(7, descriptor, xmlInstance, false, true, ""));
        yield return new("XML node invalid character", EndpointXmlSchema, descriptor => FerruleXml.SerializeEmbedded(7, descriptor, Group(Field("Id", Scalar(Text("\u0001")))), false, false, null));
        yield return new("XML node invalid schema", "{\"name\":\"Order\",\"repeating\":true,\"kind\":{\"kind\":\"group\",\"children\":[]}}",
            descriptor => FerruleXml.SerializeEmbedded(7, descriptor, xmlInstance, false, false, null));
        yield return new("XML structured", EndpointXmlSchema, descriptor => FerruleXml.ParseStructuredEmbedded(descriptor, xmlSmall));
        yield return new("XML structured bytes", EndpointXmlSchema, descriptor => FerruleXml.ParseStructuredEmbeddedBytes(descriptor, Encoding.UTF8.GetBytes(xmlSmall)));
        yield return new("XML structured root", EndpointXmlSchema, descriptor => FerruleXml.ParseStructuredEmbedded(descriptor, "<Other/>"));
        yield return new("XML structured limit before schema", "{", descriptor => FerruleXml.ParseStructuredEmbeddedBytes(descriptor, new byte[FerruleXml.MaximumInputBytes + 1]));
        yield return new("XML structured invalid schema", "{", descriptor => FerruleXml.ParseStructuredEmbedded(descriptor, xmlSmall));
        yield return new("XML observed", EndpointObservedXmlSchema, descriptor => FerruleXml.ParseEmbedded(descriptor, "<Envelope Token=\"A&amp;B\"/>", true, true));
        yield return new("XML observed policy before schema", "{", descriptor => FerruleXml.ParseEmbedded(descriptor, "<Envelope/>", true, false));
        yield return new("XML observed input", EndpointObservedXmlSchema, descriptor => FerruleXml.ParseEmbedded(descriptor, "<Envelope/>", true, true));
        yield return new("XML observed unsupported schema", EndpointXmlSchema, descriptor => FerruleXml.ParseEmbedded(descriptor, xmlSmall, true, true));
    }

    /// <summary>
    /// Shared preparations return every result and error of a fresh descriptor
    /// copy, which misses the reference-keyed preparation and is parsed again.
    /// </summary>
    private static void EndpointPreparedCodecsMatchUnpreparedCalls()
    {
        foreach (var item in EndpointCases())
        {
            var expected = EndpointOutcome(() => item.Call(UnpreparedCopy(item.Descriptor)));
            for (var call = 0; call < 2; call++)
            {
                var observed = EndpointOutcome(() => item.Call(item.Descriptor));
                if (observed != expected)
                    throw new InvalidOperationException($"{item.Name}: prepared call differs.\nexpected {expected}\nobserved {observed}");
            }
        }
    }

    /// <summary>Concurrent calls through shared preparations keep their own results and failures.</summary>
    private static void EndpointPreparedCodecsIsolateConcurrentCalls()
    {
        var cases = EndpointCases().ToArray();
        var expected = cases.Select(item => EndpointOutcome(() => item.Call(UnpreparedCopy(item.Descriptor)))).ToArray();
        var threads = Enumerable.Range(0, 8).Select(offset =>
        {
            var result = new string?[1];
            var thread = new Thread(() =>
            {
                for (var round = 0; round < 6; round++)
                    for (var index = 0; index < cases.Length; index++)
                    {
                        var position = (index + offset * 5) % cases.Length;
                        var observed = EndpointOutcome(() => cases[position].Call(cases[position].Descriptor));
                        if (observed != expected[position] && result[0] is null) result[0] = cases[position].Name;
                    }
            });
            thread.Start();
            return (thread, result);
        }).ToArray();
        foreach (var (thread, result) in threads)
        {
            thread.Join();
            Equal<string?>(null, result[0]);
        }
    }

    /// <summary>
    /// Byte routes encode the completed text without materializing it. Long
    /// supplementary-plane text splits surrogate pairs across builder chunks;
    /// every route must still return exactly the UTF-8 of its text route.
    /// </summary>
    private static void EndpointByteOutputsMatchTextOutputs()
    {
        var mapping = (Func<FerruleInstance, FerruleInstance>)EndpointJson5Mapping;
        var x12Instance = (FerruleGroup)FerruleX12.ParseEmbedded(EndpointX12Descriptor, EndpointX12Document(1));
        foreach (var prefix in new[] { "", "a", "ab", "abc" })
        {
            var note = prefix + string.Concat(Enumerable.Repeat("\U0001F642雪", 40_000));
            var json5 = "{Note: '" + note + "'}";
            var text = FerruleJson5.Execute(EndpointJson5Source, EndpointJson5Target, json5, mapping);
            Equal(Convert.ToHexString(Encoding.UTF8.GetBytes(text)),
                Convert.ToHexString(FerruleJson5.ExecuteBytes(EndpointJson5Source, EndpointJson5Target, Encoding.UTF8.GetBytes(json5), mapping)));

            var notes = Group(x12Instance.Fields.Select(field => field.Name == "NTE"
                ? Field("NTE", new FerruleRepeated([Group(Field("NTE01", Scalar(Text(note))), Field("NTE02", Scalar(Text("x"))))]))
                : field).ToArray());
            var x12 = FerruleX12.SerializeEmbedded(EndpointX12Descriptor, notes);
            Equal(Convert.ToHexString(Encoding.UTF8.GetBytes(x12)),
                Convert.ToHexString(FerruleX12.SerializeEmbeddedBytes(EndpointX12Descriptor, notes)));
        }
    }

    /// <summary>
    /// The JSON5 output byte limit applies to the counted text on both routes:
    /// exactly the maximum is returned complete, one more byte is refused with
    /// its requested size and no partial result.
    /// </summary>
    private static void EndpointJson5OutputLimitOnBothRoutes()
    {
        FerruleInstance Output(int length) => Group(Field("Note", Scalar(Text(new string('x', length)))));
        var overhead = Encoding.UTF8.GetByteCount(FerruleJson5.Execute(EndpointJson5Source, EndpointJson5Target, "{}", _ => Output(0)));
        var exact = FerruleJson.MaximumDocumentBytes - overhead;
        var text = FerruleJson5.Execute(EndpointJson5Source, EndpointJson5Target, "{}", _ => Output(exact));
        Equal(FerruleJson.MaximumDocumentBytes, text.Length);
        var bytes = FerruleJson5.ExecuteBytes(EndpointJson5Source, EndpointJson5Target, "{}"u8.ToArray(), _ => Output(exact));
        Equal(FerruleJson.MaximumDocumentBytes, bytes.Length);
        Equal(true, Encoding.UTF8.GetString(bytes) == text);
        text = null!;
        bytes = null!;
        var refusals = new (string Route, Func<object> Call)[]
        {
            ("`FerruleJson5.Execute`", () => FerruleJson5.Execute(EndpointJson5Source, EndpointJson5Target, "{}", _ => Output(exact + 1))),
            ("`FerruleJson5.ExecuteBytes`", () => FerruleJson5.ExecuteBytes(EndpointJson5Source, EndpointJson5Target, "{}"u8.ToArray(), _ => Output(exact + 1))),
        };
        if (Environment.GetEnvironmentVariable("FERRULE_ENDPOINT_PREPARATION_REPORT") == "1")
        {
            // One call each; the mapped instance is built inside the call.
            static long Allocated(Action call)
            {
                GC.Collect();
                var before = GC.GetAllocatedBytesForCurrentThread();
                call();
                return GC.GetAllocatedBytesForCurrentThread() - before;
            }
            static void Swallow(Func<object> call) { try { _ = call(); } catch (FerruleJson5BoundaryException) { } }
            Console.WriteLine($"JSON5 output limit {FerruleJson.MaximumDocumentBytes} bytes");
            Console.WriteLine("| Operation | Allocated bytes |");
            Console.WriteLine("| --- | ---: |");
            Console.WriteLine($"| `FerruleJson5.Execute`, output of exactly the limit | {Allocated(() => _ = FerruleJson5.Execute(EndpointJson5Source, EndpointJson5Target, "{}", _ => Output(exact)))} |");
            Console.WriteLine($"| `FerruleJson5.ExecuteBytes`, output of exactly the limit | {Allocated(() => _ = FerruleJson5.ExecuteBytes(EndpointJson5Source, EndpointJson5Target, "{}"u8.ToArray(), _ => Output(exact)))} |");
            foreach (var (route, call) in refusals)
                Console.WriteLine($"| {route}, output one byte above the limit | {Allocated(() => Swallow(call))} |");
        }
        foreach (var (_, route) in refusals)
        {
            try
            {
                _ = route();
                throw new InvalidOperationException("JSON5 output above the limit was returned.");
            }
            catch (FerruleJson5BoundaryException error)
            {
                Equal(FerruleJson5Stage.Output, error.Stage);
                Equal<FerruleJson5Resource?>(FerruleJson5Resource.OutputDocumentBytes, error.Resource);
                Equal<long?>(FerruleJson.MaximumDocumentBytes + 1L, error.Requested);
                Equal<long?>(FerruleJson.MaximumDocumentBytes, error.Maximum);
                Equal(true, error.InnerException is FerruleJson5ResourceException);
            }
        }
    }

    /// <summary>
    /// Allocation accounting per call. A fresh descriptor copy pays the
    /// preparation again; the shared descriptor does not. Set
    /// FERRULE_ENDPOINT_PREPARATION_REPORT=1 to print the tables.
    /// </summary>
    private static void EndpointPreparedCodecAllocationAccounting()
    {
        var report = Environment.GetEnvironmentVariable("FERRULE_ENDPOINT_PREPARATION_REPORT") == "1";
        long Allocated(int calls, Action call)
        {
            call();
            var before = GC.GetAllocatedBytesForCurrentThread();
            for (var index = 0; index < calls; index++) call();
            return (GC.GetAllocatedBytesForCurrentThread() - before) / calls;
        }
        void Row(string label, long bytes)
        {
            if (report) Console.WriteLine($"| {label} | {bytes} |");
        }
        void Header(string title)
        {
            if (!report) return;
            Console.WriteLine(title);
            Console.WriteLine("| Operation | Allocated bytes |");
            Console.WriteLine("| --- | ---: |");
        }
        // A small shared-descriptor call must pay at most this share of a
        // fresh-copy call: half, unless per-call parsing dominates.
        void Shared(string label, Action<string> call, string descriptor, double maximumShare = 0.5)
        {
            var fresh = Allocated(200, () => call(UnpreparedCopy(descriptor)));
            var shared = Allocated(200, () => call(descriptor));
            Row(label + ", fresh descriptor copy", fresh);
            Row(label + ", shared descriptor", shared);
            if (shared >= fresh * maximumShare) throw new InvalidOperationException($"{label}: shared {shared} bytes, fresh {fresh} bytes");
        }
        var mapping = (Func<FerruleInstance, FerruleInstance>)EndpointJson5Mapping;

        var json5Small = EndpointJson5Document(32);
        var json5Large = EndpointJson5Document(1_000_000);
        var json5LargeBytes = Encoding.UTF8.GetBytes(json5Large);
        Header($"JSON5: descriptors {Encoding.UTF8.GetByteCount(EndpointJson5Source)} + {Encoding.UTF8.GetByteCount(EndpointJson5Target)} bytes; small document {Encoding.UTF8.GetByteCount(json5Small)} bytes; large document {json5LargeBytes.Length} bytes");
        Shared("`FerruleJson5.Execute`, small",
            source => FerruleJson5.Execute(source, ReferenceEquals(source, EndpointJson5Source) ? EndpointJson5Target : UnpreparedCopy(EndpointJson5Target), json5Small, mapping),
            EndpointJson5Source);
        if (report)
        {
            var output = FerruleJson5.Execute(EndpointJson5Source, EndpointJson5Target, json5Large, mapping);
            Row("`FerruleJson5.Execute`, large", Allocated(3, () => FerruleJson5.Execute(EndpointJson5Source, EndpointJson5Target, json5Large, mapping)));
            Row("`FerruleJson5.ExecuteBytes`, large", Allocated(3, () => FerruleJson5.ExecuteBytes(EndpointJson5Source, EndpointJson5Target, json5LargeBytes, mapping)));
            Row("UTF-8 to UTF-16 decoding of the large input", Allocated(3, () => Encoding.UTF8.GetString(json5LargeBytes)));
            Row("UTF-16 to UTF-8 encoding of the large output", Allocated(3, () => Encoding.UTF8.GetBytes(output)));
        }

        var x12Small = EndpointX12Document(2);
        var x12SmallInstance = FerruleX12.ParseEmbedded(EndpointX12Descriptor, x12Small);
        Header($"X12: descriptor {Encoding.UTF8.GetByteCount(EndpointX12Descriptor)} bytes; small document {x12Small.Length} bytes");
        Shared("`FerruleX12.ParseEmbedded`, small", descriptor => FerruleX12.ParseEmbedded(descriptor, x12Small), EndpointX12Descriptor);
        Shared("`FerruleX12.SerializeEmbedded`, small", descriptor => FerruleX12.SerializeEmbedded(descriptor, x12SmallInstance), EndpointX12Descriptor);
        if (report)
        {
            var large = EndpointX12Document(20_000);
            var largeBytes = Encoding.UTF8.GetBytes(large);
            var instance = FerruleX12.ParseEmbeddedBytes(EndpointX12Descriptor, largeBytes);
            var output = FerruleX12.SerializeEmbedded(EndpointX12Descriptor, instance);
            Console.WriteLine($"large document {largeBytes.Length} bytes; large output {Encoding.UTF8.GetByteCount(output)} bytes");
            Row("`FerruleX12.ParseEmbeddedBytes`, large", Allocated(3, () => FerruleX12.ParseEmbeddedBytes(EndpointX12Descriptor, largeBytes)));
            Row("UTF-8 to UTF-16 decoding of the large input", Allocated(3, () => Encoding.UTF8.GetString(largeBytes)));
            Row("`FerruleX12.SerializeEmbedded`, large", Allocated(3, () => FerruleX12.SerializeEmbedded(EndpointX12Descriptor, instance)));
            Row("`FerruleX12.SerializeEmbeddedBytes`, large", Allocated(3, () => FerruleX12.SerializeEmbeddedBytes(EndpointX12Descriptor, instance)));
            Row("UTF-16 to UTF-8 encoding of the large output", Allocated(3, () => Encoding.UTF8.GetBytes(output)));
        }

        var xmlSmallInstance = EndpointXmlInstance(3);
        var xmlSmall = FerruleXml.SerializeDocumentEmbedded(EndpointXmlSchema, xmlSmallInstance, true, true, null, null);
        Header($"XML: descriptor {Encoding.UTF8.GetByteCount(EndpointXmlSchema)} bytes; small document {Encoding.UTF8.GetByteCount(xmlSmall)} bytes");
        Shared("`FerruleXml.ParseStructuredEmbedded`, small", descriptor => FerruleXml.ParseStructuredEmbedded(descriptor, xmlSmall), EndpointXmlSchema);
        Shared("`FerruleXml.ParseEmbedded` (observed flat root), small",
            descriptor => FerruleXml.ParseEmbedded(descriptor, "<Envelope Token=\"A&amp;B\"/>", true, true), EndpointObservedXmlSchema,
            maximumShare: 0.75);
        Shared("`FerruleXml.SerializeDocumentEmbedded`, small",
            descriptor => FerruleXml.SerializeDocumentEmbedded(descriptor, xmlSmallInstance, true, true, null, null), EndpointXmlSchema);
        Shared("`FerruleXml.SerializeEmbedded` (expression node), small",
            descriptor => FerruleXml.SerializeEmbedded(7, descriptor, xmlSmallInstance, false, false, null), EndpointXmlSchema);
        if (report)
        {
            var instance = EndpointXmlInstance(20_000);
            var large = FerruleXml.SerializeDocumentEmbedded(EndpointXmlSchema, instance, true, true, null, null);
            var largeBytes = Encoding.UTF8.GetBytes(large);
            Console.WriteLine($"large document {largeBytes.Length} bytes");
            Row("`FerruleXml.ParseStructuredEmbeddedBytes`, large", Allocated(3, () => FerruleXml.ParseStructuredEmbeddedBytes(EndpointXmlSchema, largeBytes)));
            Row("UTF-8 to UTF-16 decoding of the large input", Allocated(3, () => Encoding.UTF8.GetString(largeBytes)));
            Row("`FerruleXml.SerializeDocumentEmbedded`, large", Allocated(3, () => FerruleXml.SerializeDocumentEmbedded(EndpointXmlSchema, instance, true, true, null, null)));
            Row("`FerruleXml.PrepareDocumentEmbeddedUtf8` and `ToByteArray`, large",
                Allocated(3, () => FerruleXml.PrepareDocumentEmbeddedUtf8(EndpointXmlSchema, instance, true, true, null, null).ToByteArray()));
        }
    }
}
