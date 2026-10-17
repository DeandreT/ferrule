using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void Json5FinalNewlineAndOrdinaryWriter()
    {
        var cases = new List<(string Input, string Expected)>
        {
            ("{n:''}", "{\n  \"n\": \"\"\n}\n"),
            ("{n:'\\u0000\\b\\f\\n\\r\\t\"\\\\/雪😀\\u2028\\u2029'}",
             "{\n  \"n\": \"\\u0000\\b\\f\\n\\r\\t\\\"\\\\/雪😀\u2028\u2029\"\n}\n"),
        };
        foreach (var count in new[] { 0, 7_999, 8_000, 8_001, 16_383 })
        {
            var padding = new string('x', count);
            cases.Add(("{n:'" + padding + "😀雪\\u2028\\u2029'}",
                "{\n  \"n\": \"" + padding + "😀雪\u2028\u2029\"\n}\n"));
        }
        var utf8 = new UTF8Encoding(false, true);
        var descriptor = Json5Descriptor("S");
        for (var index = 0; index < cases.Count; index++)
        {
            var (input, expected) = cases[index];
            var expectedBytes = utf8.GetBytes(expected);
            foreach (var bytes in new[] { false, true })
            {
                var observed = Json5Observe($"final-newline-{index}-{bytes}", input,
                    utf8.GetBytes(input), descriptor, descriptor, bytes);
                Equal((Exception?)null, observed.Error);
                Equal(1, observed.MappingCalls);
                if (bytes) { Equal(true, observed.Bytes!.AsSpan().SequenceEqual(expectedBytes)); }
                else { Equal(expected, observed.Text); }

                // The independent literal includes exactly one document newline.
                // Exercise the unchanged ordinary writer wrapper with the same
                // complete typed value, including controls and supplementary text.
                var ordinary = FerruleJson.SerializeEmbedded(descriptor, observed.Typed!);
                Equal(expected, ordinary);
                Equal(true, FerruleJson.SerializeEmbeddedBytes(descriptor, observed.Typed!)
                    .AsSpan().SequenceEqual(expectedBytes));
            }
        }
    }
}
