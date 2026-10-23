using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string JsonRouteStringSchema = "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";

    private const string JsonRouteObjectSchema =
        "{\"name\":\"Order\",\"kind\":{\"kind\":\"group\",\"children\":[" +
        "{\"name\":\"Id\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}," +
        "{\"name\":\"Line\",\"repeating\":true,\"kind\":{\"kind\":\"group\",\"children\":[" +
        "{\"name\":\"Note\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}]}}";

    /// <summary>Every ordinary JSON output route with its text twin, so byte routes can be compared to them.</summary>
    private static readonly (string Name, Func<string, FerruleInstance, string> Text, Func<string, FerruleInstance, byte[]> Bytes)[] JsonOutputRoutes =
    [
        ("Serialize", FerruleJson.Serialize, FerruleJson.SerializeBytes),
        ("SerializeEmbedded", FerruleJson.SerializeEmbedded, FerruleJson.SerializeEmbeddedBytes),
        ("SerializeEmbeddedCompact", FerruleJson.SerializeEmbeddedCompact, FerruleJson.SerializeEmbeddedCompactBytes),
    ];

    /// <summary>
    /// Byte routes encode the completed text without materializing it. Long
    /// supplementary-plane text splits surrogate pairs across builder chunks;
    /// every byte route must still return exactly the UTF-8 of its text route.
    /// </summary>
    private static void JsonByteOutputsMatchTextOutputs()
    {
        foreach (var prefix in new[] { "", "a", "ab", "abc" })
        {
            var note = prefix + string.Concat(Enumerable.Repeat("\U0001F642雪\u2028\"\\", 30_000));
            var instances = new (string Schema, FerruleInstance Value)[]
            {
                (JsonRouteStringSchema, Scalar(Text(note))),
                (JsonRouteObjectSchema, Group(
                    Field("Id", Scalar(Text(prefix + "\U0001F642"))),
                    Field("Line", new FerruleRepeated(Enumerable.Range(0, 3)
                        .Select(index => (FerruleInstance)Group(Field("Note", Scalar(Text(new string('y', index) + note))))).ToArray())))),
            };
            foreach (var (schema, value) in instances)
            {
                foreach (var (name, text, bytes) in JsonOutputRoutes)
                {
                    var expected = EndpointOutcome(() => Encoding.UTF8.GetBytes(text(schema, value)));
                    var observed = EndpointOutcome(() => bytes(schema, value));
                    if (observed != expected) throw new InvalidOperationException($"{name}: byte route differs from its text route.");
                }
            }
        }
    }

    /// <summary>
    /// The output byte limit applies to the counted text on every route: an
    /// output of exactly the limit is returned complete, and one more byte is
    /// refused with the same error by the text and byte routes.
    /// </summary>
    private static void JsonOutputLimitOnTextAndByteRoutes()
    {
        var report = Environment.GetEnvironmentVariable("FERRULE_JSON_PREPARATION_REPORT") == "1";
        if (report)
        {
            Console.WriteLine($"JSON output limit {FerruleJson.MaximumDocumentBytes} bytes");
            Console.WriteLine("| Operation | Allocated bytes |");
            Console.WriteLine("| --- | ---: |");
        }
        // A string scalar adds its quotes, and the pretty routes a final newline.
        foreach (var (name, text, bytes) in JsonOutputRoutes)
        {
            var framing = name.EndsWith("Compact", StringComparison.Ordinal) ? 2 : 3;
            var exact = FerruleJson.MaximumDocumentBytes - framing;
            var accepted = text(JsonRouteStringSchema, Scalar(Text(new string('x', exact))));
            Equal(FerruleJson.MaximumDocumentBytes, accepted.Length);
            accepted = null!;
            var acceptedBytes = bytes(JsonRouteStringSchema, Scalar(Text(new string('x', exact))));
            Equal(FerruleJson.MaximumDocumentBytes, acceptedBytes.Length);
            acceptedBytes = null!;
            var over = Scalar(Text(new string('x', exact + 1)));
            var refusedText = EndpointOutcome(() => text(JsonRouteStringSchema, over));
            Equal(true, refusedText.Contains($"JSON output is {FerruleJson.MaximumDocumentBytes + 1} bytes", StringComparison.Ordinal));
            Equal(refusedText, EndpointOutcome(() => bytes(JsonRouteStringSchema, over)));
            if (!report) continue;
            static long Allocated(Action call)
            {
                GC.Collect();
                var before = GC.GetAllocatedBytesForCurrentThread();
                call();
                return GC.GetAllocatedBytesForCurrentThread() - before;
            }
            static void Swallow(Func<object> call) { try { _ = call(); } catch (FerruleRuntimeException) { } }
            var exactInstance = Scalar(Text(new string('x', exact)));
            Console.WriteLine($"| `{name}`, output of exactly the limit | {Allocated(() => _ = text(JsonRouteStringSchema, exactInstance))} |");
            Console.WriteLine($"| `{name}Bytes`, output of exactly the limit | {Allocated(() => _ = bytes(JsonRouteStringSchema, exactInstance))} |");
            Console.WriteLine($"| `{name}`, output one byte above the limit | {Allocated(() => Swallow(() => text(JsonRouteStringSchema, over)))} |");
            Console.WriteLine($"| `{name}Bytes`, output one byte above the limit | {Allocated(() => Swallow(() => bytes(JsonRouteStringSchema, over)))} |");
        }
    }
}
