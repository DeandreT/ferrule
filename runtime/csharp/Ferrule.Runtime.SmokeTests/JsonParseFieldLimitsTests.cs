using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string RawIntSchema =
        """{"name":"Value","kind":{"kind":"scalar","ty":"int"}}""";

    private static void JsonParseFieldNativeLimits()
    {
        const string schemaPrefix = "{\"name\":\"";
        const string schemaSuffix = "\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
        foreach (var length in new[]
        {
            FerruleJson.MaximumSchemaBytes,
            FerruleJson.MaximumSchemaBytes + 1,
        })
        {
            var schema = schemaPrefix +
                new string('N', length - schemaPrefix.Length - schemaSuffix.Length) +
                schemaSuffix;
            Equal(length, Encoding.UTF8.GetByteCount(schema));
            Equal(
                FerruleValue.FromInt64(42),
                ParseField("42", schema, "[]"));
            if (length > FerruleJson.MaximumSchemaBytes)
            {
                Error(
                    FerruleRuntimeError.JsonBoundary,
                    () => FerruleJson.Parse(schema, "42"));
                Error(
                    FerruleRuntimeError.JsonBoundary,
                    () => FerruleJson.Serialize(
                        schema,
                        new FerruleScalar(FerruleValue.FromInt64(42))));
            }
        }

        var largeInput = "42" + new string(' ', FerruleJson.MaximumDocumentBytes);
        Equal(FerruleJson.MaximumDocumentBytes + 2, largeInput.Length);
        Equal(FerruleValue.FromInt64(42), ParseField(largeInput, RawIntSchema, "[]"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(RawIntSchema, largeInput));

        const string rowsSchema =
            """{"name":"Root","kind":{"kind":"group","children":[{"name":"Rows","repeating":true,"kind":{"kind":"group","children":[{"name":"v","kind":{"kind":"scalar","ty":"int"}}]}},{"name":"Count","kind":{"kind":"scalar","ty":"int"}}]}}""";
        var document = new StringBuilder(8_000_032);
        document.Append("{\"Rows\":[{\"v\":1}");
        for (var index = 1; index < FerruleJson.MaximumNodes + 1; index++)
        {
            document.Append(",{\"v\":1}");
        }
        document.Append("],\"Count\":7}");
        var manyMappedNodes = document.ToString();
        Equal(
            FerruleValue.FromInt64(7),
            ParseField(manyMappedNodes, rowsSchema, "[\"Count\"]"));

        ExpectParseFieldError(
            "schema descriptor is invalid",
            "not a schema", "not a path", "not JSON");
        ExpectParseFieldError(
            "field path descriptor is invalid",
            RawIntSchema, "not a path", "not JSON");
        ExpectParseFieldError(
            "input does not match the JSON schema",
            RawIntSchema, "[]", "not JSON");
        Equal(
            FerruleValue.Null,
            FerruleFunctions.Call("json_parse_field", new[]
            {
                FerruleValue.Null, Text("not a schema"), Text("not a path"),
            }));
        Equal(
            FerruleValue.Null,
            FerruleFunctions.Call("json_parse_field", new[]
            {
                FerruleValue.JsonNull, Text("not a schema"), Text("not a path"),
            }));
    }

    private static FerruleValue ParseField(string input, string schema, string path) =>
        FerruleFunctions.Call("json_parse_field", new[]
        {
            Text(input), Text(schema), Text(path),
        });

    private static void ExpectParseFieldError(
        string detail,
        string schema,
        string path,
        string input)
    {
        var error = Error(
            FerruleRuntimeError.FunctionInvalidArgument,
            () => ParseField(input, schema, path));
        if (!error.Message.Contains(detail, StringComparison.Ordinal))
        {
            throw new InvalidOperationException(
                $"Expected json_parse_field detail {detail}, got {error.Message}");
        }
    }
}
