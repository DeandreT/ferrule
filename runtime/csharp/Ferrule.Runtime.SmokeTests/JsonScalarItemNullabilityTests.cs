using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static string ScalarItemSchema(bool outer, bool item)
    {
        var fields = new Dictionary<string, object?>
        {
            ["name"] = "Items", ["repeating"] = true,
            ["kind"] = new { kind = "scalar", ty = "int" },
        };
        if (outer) fields["container_nullable"] = true;
        if (item) fields["nullable"] = true;
        return JsonSerializer.Serialize(fields);
    }

    // A nullable scalar array keeps its items' own value domain (#924).
    private static void JsonScalarItemNullability()
    {
        var originals = new List<(string Schema, string Input, string Api, string? ExpectedMessage, string? ExpectedOutput, FerruleInstance? Actual, string? Output, Exception? Error)>();
        foreach (var (outer, item) in new[] { (false, false), (false, true), (true, false), (true, true) })
        {
            var schema = ScalarItemSchema(outer, item);
            (string Input, bool Accepted, string Expected, string Got, string Output)[] controls =
            [
                ("null", outer, "array", "Null", "null\n"),
                ("[]", true, "", "", "[]\n"),
                ("[1]", true, "", "", "[\n  1\n]\n"),
                ("[null]", item, "integer", "Null", "[\n  null\n]\n"),
                ("[1,null]", item, "integer", "Null", "[\n  1,\n  null\n]\n"),
                ("{}", false, "array", "Object", ""),
                ("[true]", false, "integer", "True", ""),
            ];
            foreach (var control in controls)
            {
                foreach (var api in new[] { "text", "bytes" })
                {
                    FerruleInstance? actual = null;
                    string? output = null;
                    Exception? error = null;
                    try
                    {
                        actual = api == "text" ? FerruleJson.ParseEmbedded(schema, control.Input)
                            : FerruleJson.ParseEmbeddedBytes(schema, Encoding.UTF8.GetBytes(control.Input));
                        output = api == "text" ? FerruleJson.SerializeEmbedded(schema, actual)
                            : Encoding.UTF8.GetString(FerruleJson.SerializeEmbeddedBytes(schema, actual));
                    }
                    catch (Exception original) { error = original; }
                    originals.Add((schema, control.Input, api,
                        control.Accepted ? null : $"JSON field 'Items' expected {control.Expected}, got {control.Got}.",
                        control.Accepted ? control.Output : null, actual, output, error));
                }
            }
            var nullItems = new FerruleRepeated(new FerruleInstance[] { new FerruleScalar(FerruleValue.FromInt64(1)), new FerruleScalar(FerruleValue.JsonNull) });
            foreach (var api in new[] { "text", "bytes" })
            {
                string? output = null;
                Exception? error = null;
                try
                {
                    output = api == "text" ? FerruleJson.SerializeEmbedded(schema, nullItems)
                        : Encoding.UTF8.GetString(FerruleJson.SerializeEmbeddedBytes(schema, nullItems));
                }
                catch (Exception original) { error = original; }
                originals.Add((schema, "output [1,null]", api,
                    item ? null : "JSON field 'Items' expected integer, got JsonNull.",
                    item ? "[\n  1,\n  null\n]\n" : null, null, output, error));
            }
        }
        Console.Error.WriteLine("SCALAR_ITEM_NULLABILITY_ORIGINALS " + JsonSerializer.Serialize(originals.Select(original => new
        {
            original.Schema, original.Input, original.Api, original.ExpectedMessage, original.Output,
            actual = JsonRequiredPrecedenceInstance(original.Actual), error = JsonRequiredPrecedenceError(original.Error),
        })));
        Console.Error.Flush();
        foreach (var original in originals)
        {
            if (original.ExpectedMessage is { } message)
            {
                if (original.Error is not FerruleRuntimeException error) throw new InvalidOperationException("Expected scalar-item boundary refusal: " + original.Input);
                Equal(FerruleRuntimeError.JsonBoundary, error.Error);
                Equal(message, error.Message);
                Equal(message, error.Detail);
                Equal<Exception?>(null, error.InnerException);
                Equal<string?>(null, original.Output);
            }
            else
            {
                Equal<Exception?>(null, original.Error);
                Equal(original.ExpectedOutput, original.Output);
            }
        }
    }
}
