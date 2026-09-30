using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void JsonOutputPredicateCanonicalByteLimit()
    {
        // Utf8JsonWriter escapes U+2028, making the temporary item/object
        // and unique key larger than 64 MiB, while the canonical JSON/key
        // stays under 64 MiB.
        var text = new string('\u2028', 11_200_000);
        const string contains =
            """{"name":"Values","repeating":true,"json_unique_items":true,"json_contains":[{"predicate":{"kind":"schema","schema":{"name":"item","kind":{"kind":"scalar","ty":"string"}}},"range":{"minimum":1}}],"kind":{"kind":"scalar","ty":"string"}}""";
        var repeated = new FerruleRepeated(new FerruleInstance[]
        {
            Scalar(Text(text)),
        });
        var output = FerruleJson.Serialize(contains, repeated);
        Equal(true, System.Text.Encoding.UTF8.GetByteCount(output) < FerruleJson.MaximumDocumentBytes);
        Equal(true, output.Contains(text, StringComparison.Ordinal));
        _ = FerruleJson.Parse(contains, output);

        const string dependent =
            """{"name":"Root","json_dependent_schemas":[{"trigger":"Trigger","predicate":{"kind":"schema","schema":{"name":"predicate","kind":{"kind":"group","children":[{"name":"Trigger","kind":{"kind":"scalar","ty":"bool"}},{"name":"Text","kind":{"kind":"scalar","ty":"string"}}],"required":["Text"]}}}}],"kind":{"kind":"group","children":[{"name":"Trigger","kind":{"kind":"scalar","ty":"bool"}},{"name":"Text","kind":{"kind":"scalar","ty":"string"}}]}}""";
        output = FerruleJson.Serialize(
            dependent,
            Group(
                Field("Trigger", Scalar(FerruleValue.FromBoolean(true))),
                Field("Text", Scalar(Text(text)))));
        Equal(true, System.Text.Encoding.UTF8.GetByteCount(output) < FerruleJson.MaximumDocumentBytes);
        Equal(true, output.Contains(text, StringComparison.Ordinal));
    }

    private static void JsonOutputPredicateNumericProvenance()
    {
        // serde_json writes these adjacent values as 1e-307 and
        // 1.0000000000000001e-307, then reparses those texts to the opposite
        // double. Output predicates must inspect the original normalized value.
        var low = BitConverter.Int64BitsToDouble(0x0031fa182c40c60d);
        var high = BitConverter.Int64BitsToDouble(0x0031fa182c40c60e);

        const string minimum =
            """{"kind":"number","bounds":{"minimum":{"value":1e-307}}}""";
        var contains =
            """{"name":"Values","repeating":true,"json_contains":[{"predicate":{"kind":"schema","schema":{"name":"item","numeric_range":__MINIMUM__,"kind":{"kind":"scalar","ty":"float"}}},"range":{"minimum":1}}],"kind":{"kind":"scalar","ty":"float"}}"""
                .Replace("__MINIMUM__", minimum, StringComparison.Ordinal);
        FerruleInstance values(double value) =>
            new FerruleRepeated(new FerruleInstance[]
            {
                Scalar(FerruleValue.FromDouble(value)),
            });

        var containsError = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(contains, values(low)));
        Equal(true, containsError.Message.Contains("contains predicate", StringComparison.Ordinal));
        Equal(
            true,
            FerruleJson.Serialize(contains, values(high))
                .Contains("1.0000000000000001e-307", StringComparison.Ordinal));
        _ = FerruleJson.Parse(contains, "[1e-307]");
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(contains, "[1.0000000000000001e-307]"));

        var dependent =
            """{"name":"Root","json_dependent_schemas":[{"trigger":"Trigger","predicate":{"kind":"schema","schema":{"name":"predicate","kind":{"kind":"group","children":[{"name":"Trigger","kind":{"kind":"scalar","ty":"bool"}},{"name":"Value","numeric_range":__MINIMUM__,"kind":{"kind":"scalar","ty":"float"}}],"required":["Value"]}}}}],"kind":{"kind":"group","children":[{"name":"Trigger","kind":{"kind":"scalar","ty":"bool"}},{"name":"Value","kind":{"kind":"scalar","ty":"float"}}]}}"""
                .Replace("__MINIMUM__", minimum, StringComparison.Ordinal);
        FerruleInstance record(double value) => Group(
            Field("Trigger", Scalar(FerruleValue.FromBoolean(true))),
            Field("Value", Scalar(FerruleValue.FromDouble(value))));

        var dependentError = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(dependent, record(low)));
        Equal(true, dependentError.Message.Contains("dependent schema", StringComparison.Ordinal));
        Equal(
            true,
            FerruleJson.Serialize(dependent, record(high))
                .Contains("1.0000000000000001e-307", StringComparison.Ordinal));
        _ = FerruleJson.Parse(dependent, "{\"Trigger\":true,\"Value\":1e-307}");
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                dependent,
                "{\"Trigger\":true,\"Value\":1.0000000000000001e-307}"));

        const string exactAlternative =
            """{"name":"Root","json_dependent_schemas":[{"trigger":"Trigger","predicate":{"kind":"schema","schema":{"name":"predicate","kind":{"kind":"group","children":[{"name":"Trigger","kind":{"kind":"scalar","ty":"bool"}},{"name":"Value","kind":{"kind":"scalar","ty":"float"}}],"alternatives":[{"members":["Trigger","Value"],"required":["Value"],"constraints":[{"member":"Value","value":{"type":"float","value":1e-307}}]}]}}}}],"kind":{"kind":"group","children":[{"name":"Trigger","kind":{"kind":"scalar","ty":"bool"}},{"name":"Value","kind":{"kind":"scalar","ty":"float"}}]}}""";
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(exactAlternative, record(low)));
        _ = FerruleJson.Serialize(exactAlternative, record(high));
        _ = FerruleJson.Parse(
            exactAlternative,
            "{\"Trigger\":true,\"Value\":1e-307}");
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                exactAlternative,
                "{\"Trigger\":true,\"Value\":1.0000000000000001e-307}"));
    }
}
