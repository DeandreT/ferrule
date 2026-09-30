using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void JsonLargeCollectionsMatchPublicBoundaryLimits()
    {
        const int rowCount = 1_000_001;
        const string groupedRowsSchema =
            """{"name":"Rows","repeating":true,"kind":{"kind":"group","children":[{"name":"v","kind":{"kind":"scalar","ty":"int"}}]}}""";

        static void LargeGroupedCollection()
        {
            var document = "[" + string.Join(',', Enumerable.Repeat("{\"v\":0}", rowCount)) + "]";
            Equal(8_000_009, Encoding.UTF8.GetByteCount(document));
            Equal(true, document.Length < FerruleJson.MaximumDocumentBytes);
            var parsed = (FerruleRepeated)FerruleJson.Parse(groupedRowsSchema, document);
            Equal(rowCount, parsed.Items.Count);
            Equal(FerruleValue.FromInt64(0), ScalarPathResolver.Resolve(parsed.Items[0], "v"));
            Equal(FerruleValue.FromInt64(0), ScalarPathResolver.Resolve(parsed.Items[^1], "v"));

            var output = FerruleJson.Serialize(groupedRowsSchema, parsed);
            Equal(20_000_023, Encoding.UTF8.GetByteCount(output));
            Equal(true, output.Length < FerruleJson.MaximumDocumentBytes);
            using var rendered = JsonDocument.Parse(output);
            Equal(rowCount, rendered.RootElement.GetArrayLength());
            Equal(0L, rendered.RootElement[0].GetProperty("v").GetInt64());
            Equal(0L, rendered.RootElement[rowCount - 1].GetProperty("v").GetInt64());
        }

        static void PublicByteLimits()
        {
            const string integerSchema =
                """{"name":"Value","kind":{"kind":"scalar","ty":"int"}}""";
            var schema = integerSchema + new string(' ', FerruleJson.MaximumSchemaBytes + 1 - integerSchema.Length);
            Equal(
                true,
                Error(
                    FerruleRuntimeError.JsonBoundary,
                    () => FerruleJson.Parse(schema, "0"))
                    .Message.Contains("embedded JSON schema", StringComparison.Ordinal));
            Equal(
                true,
                Error(
                    FerruleRuntimeError.JsonBoundary,
                    () => FerruleJson.Serialize(schema, Scalar(FerruleValue.FromInt64(0))))
                    .Message.Contains("embedded JSON schema", StringComparison.Ordinal));

            var input = new byte[FerruleJson.MaximumDocumentBytes + 1];
            Array.Fill(input, (byte)' ');
            input[0] = (byte)'0';
            Equal(
                true,
                Error(
                    FerruleRuntimeError.JsonBoundary,
                    () => FerruleJson.ParseBytes(integerSchema, input))
                    .Message.Contains($"maximum is {FerruleJson.MaximumDocumentBytes}", StringComparison.Ordinal));

            const string stringSchema =
                """{"name":"Value","kind":{"kind":"scalar","ty":"string"}}""";
            // Escaping each backslash doubles the wire size. The quote pair
            // and trailing newline put this document just over 64 MiB.
            var value = Scalar(Text(new string('\\', FerruleJson.MaximumDocumentBytes / 2)));
            Equal(
                true,
                Error(
                    FerruleRuntimeError.JsonBoundary,
                    () => FerruleJson.Serialize(stringSchema, value))
                    .Message.Contains("JSON output", StringComparison.Ordinal));
        }

        static void ContainsDoesNotAddAnArrayItemCap()
        {
            const string schema =
                """{"name":"Values","repeating":true,"json_contains":[{"predicate":{"kind":"schema","schema":{"name":"any","json_any":true,"kind":{"kind":"scalar","ty":"string"}}},"range":{"minimum":1}}],"kind":{"kind":"scalar","ty":"int"}}""";
            var document = "[" + string.Join(',', Enumerable.Repeat("0", rowCount)) + "]";
            var parsed = (FerruleRepeated)FerruleJson.Parse(schema, document);
            Equal(rowCount, parsed.Items.Count);
            var output = FerruleJson.Serialize(schema, parsed);
            Equal(5_000_008, Encoding.UTF8.GetByteCount(output));
            using var rendered = JsonDocument.Parse(output);
            Equal(rowCount, rendered.RootElement.GetArrayLength());
            Equal(0L, rendered.RootElement[rowCount - 1].GetInt64());
        }

        static void UniqueItemsRetainsItsOwnBudget()
        {
            const string uniqueSchema =
                """{"name":"Values","repeating":true,"json_unique_items":true,"kind":{"kind":"scalar","ty":"int"}}""";
            var document = new StringBuilder(rowCount * 8);
            document.Append('[');
            for (var item = 0; item < rowCount; item++)
            {
                if (item != 0)
                {
                    document.Append(',');
                }
                document.Append(item);
            }
            document.Append(']');
            var text = document.ToString();
            Equal(true, text.Length < FerruleJson.MaximumDocumentBytes);
            var error = Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(uniqueSchema, text));
            Equal(true, error.Message.Contains("uniqueItems", StringComparison.Ordinal));
            Equal(true, error.Message.Contains("1000000", StringComparison.Ordinal));
        }

        LargeGroupedCollection();
        ContainsDoesNotAddAnArrayItemCap();
        PublicByteLimits();
        UniqueItemsRetainsItsOwnBudget();
    }
}
