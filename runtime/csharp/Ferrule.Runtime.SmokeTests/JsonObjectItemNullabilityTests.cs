using System.Reflection;
using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private sealed record ObjectItemOriginal(string Schema, string Input, string Api,
        FerruleInstance? Expected, string? ExpectedMessage, FerruleInstance? Actual, Exception? Error);

    private static string ObjectItemSchema(bool outer, bool item, bool explicitItem = true)
    {
        var fields = new Dictionary<string, object?>
        {
            ["name"] = "Items", ["repeating"] = true,
            ["kind"] = new { kind = "group", children = Array.Empty<object>() },
        };
        if (outer) fields["container_nullable"] = true;
        if (explicitItem && (outer || item)) fields["json_object_item_nullable"] = item;
        return (explicitItem && (outer || item) ? "FERRULE-EMBEDDED-SCHEMA/4\n" : "")
            + JsonSerializer.Serialize(fields);
    }

    private static void JsonObjectItemNullability()
    {
        var originals = new List<ObjectItemOriginal>();
        foreach (var (outer, item) in new[] { (false, false), (false, true), (true, false), (true, true) })
        {
            var schema = ObjectItemSchema(outer, item);
            FerruleInstance EmptyObject() => new FerruleGroup(Array.Empty<FerruleField>());
            (string Input, FerruleInstance? Expected, string? ExpectedMessage)[] controls =
            [
                ("null", outer ? new FerruleScalar(FerruleValue.JsonNull) : null,
                    outer ? null : "JSON field 'Items' expected array, got Null."),
                ("[]", new FerruleRepeated(Array.Empty<FerruleInstance>()), null),
                ("{}", null, "JSON field 'Items' expected array, got Object."),
                ("[null]", item ? new FerruleRepeated(new FerruleInstance[] { new FerruleScalar(FerruleValue.JsonNull) }) : null,
                    item ? null : "JSON field 'Items' expected object, got Null."),
                ("[{}]", new FerruleRepeated(new[] { EmptyObject() }), null),
                ("[null,{}]", item ? new FerruleRepeated(new FerruleInstance[] { new FerruleScalar(FerruleValue.JsonNull), EmptyObject() }) : null,
                    item ? null : "JSON field 'Items' expected object, got Null."),
                ("[false]", null, "JSON field 'Items' expected object, got False."),
                ("[1]", null, "JSON field 'Items' expected object, got Number."),
                ("[\"null\"]", null, "JSON field 'Items' expected object, got String."),
            ];
            foreach (var control in controls)
            {
                foreach (var api in new[] { "text", "bytes" })
                {
                    FerruleInstance? actual = null;
                    Exception? error = null;
                    try
                    {
                        actual = api == "text" ? FerruleJson.ParseEmbedded(schema, control.Input)
                            : FerruleJson.ParseEmbeddedBytes(schema, Encoding.UTF8.GetBytes(control.Input));
                    }
                    catch (Exception original) { error = original; }
                    originals.Add(new(schema, control.Input, api, control.Expected, control.ExpectedMessage, actual, error));
                }
            }
        }
        Console.Error.WriteLine("OBJECT_ITEM_NULLABILITY_ORIGINALS " + JsonSerializer.Serialize(originals.Select(original => new
        {
            original.Schema, original.Input, original.Api, original.ExpectedMessage,
            actual = JsonRequiredPrecedenceInstance(original.Actual), error = JsonRequiredPrecedenceError(original.Error),
        })));
        Console.Error.Flush();
        foreach (var original in originals)
        {
            if (original.ExpectedMessage is { } message)
            {
                if (original.Error is not FerruleRuntimeException error) throw new InvalidOperationException("Expected JSON boundary refusal.");
                Equal(FerruleRuntimeError.JsonBoundary, error.Error);
                Equal(message, error.Message);
                Equal(message, error.Detail);
                Equal<Exception?>(null, error.InnerException);
                Equal<FerruleInstance?>(null, original.Actual);
                foreach (var property in typeof(FerruleRuntimeException).GetProperties(BindingFlags.Instance | BindingFlags.Public | BindingFlags.DeclaredOnly))
                    if (property.Name is not (nameof(FerruleRuntimeException.Error) or nameof(FerruleRuntimeException.Detail)))
                        Equal<object?>(null, property.GetValue(error));
            }
            else
            {
                Equal<Exception?>(null, original.Error);
                Equal(JsonSerializer.Serialize(JsonRequiredPrecedenceInstance(original.Expected)),
                    JsonSerializer.Serialize(JsonRequiredPrecedenceInstance(original.Actual)));
                var expected = original.Input switch
                {
                    "null" => "null\n", "[]" => "[]\n", "[null]" => "[\n  null\n]\n",
                    "[{}]" => "[\n  {}\n]\n", "[null,{}]" => "[\n  null,\n  {}\n]\n",
                    _ => throw new InvalidOperationException("Undeclared successful control."),
                };
                Equal(expected, FerruleJson.SerializeEmbedded(original.Schema, original.Expected!));
                Equal(expected, Encoding.UTF8.GetString(FerruleJson.SerializeEmbeddedBytes(original.Schema, original.Expected!)));
            }
        }
    }

    private static void JsonObjectItemOutputNullability()
    {
        var originals = new List<(string Schema, string Api, string? ExpectedMessage, string? ExpectedOutput, string? Output, Exception? Error)>();
        foreach (var (outer, item) in new[] { (false, false), (false, true), (true, false), (true, true) })
        {
            var schema = ObjectItemSchema(outer, item);
            foreach (var (instance, accepted, expectedKind, expectedOutput) in new (FerruleInstance, bool, string, string)[]
            {
                (new FerruleScalar(FerruleValue.JsonNull), outer, "array", "null\n"),
                (new FerruleRepeated(new FerruleInstance[] { new FerruleScalar(FerruleValue.JsonNull) }), item, "object", "[\n  null\n]\n"),
            })
            {
                foreach (var api in new[] { "text", "bytes" })
                {
                    string? output = null;
                    Exception? error = null;
                    try { output = api == "text" ? FerruleJson.SerializeEmbedded(schema, instance) : Encoding.UTF8.GetString(FerruleJson.SerializeEmbeddedBytes(schema, instance)); }
                    catch (Exception original) { error = original; }
                    originals.Add((schema, api, accepted ? null : $"JSON field 'Items' expected {expectedKind}, got JsonNull.", accepted ? expectedOutput : null, output, error));
                }
            }
        }
        Console.Error.WriteLine("OBJECT_ITEM_OUTPUT_ORIGINALS " + JsonSerializer.Serialize(originals.Select(original => new
        {
            original.Schema, original.Api, original.ExpectedMessage, original.ExpectedOutput, original.Output, error = JsonRequiredPrecedenceError(original.Error),
        })));
        Console.Error.Flush();
        foreach (var original in originals)
        {
            Equal(original.ExpectedOutput, original.Output);
            if (original.ExpectedMessage is { } message)
            {
                if (original.Error is not FerruleRuntimeException error) throw new InvalidOperationException("Expected object-item output refusal.");
                Equal(FerruleRuntimeError.JsonBoundary, error.Error);
                Equal(message, error.Message); Equal(message, error.Detail); Equal<Exception?>(null, error.InnerException);
                foreach (var property in typeof(FerruleRuntimeException).GetProperties(BindingFlags.Instance | BindingFlags.Public | BindingFlags.DeclaredOnly))
                    if (property.Name is not (nameof(FerruleRuntimeException.Error) or nameof(FerruleRuntimeException.Detail))) Equal<object?>(null, property.GetValue(error));
            }
            else Equal<Exception?>(null, original.Error);
        }
    }

    private static void JsonObjectItemMetadataGates()
    {
        var legacy = ObjectItemSchema(true, false, explicitItem: false);
        var retained = FerruleJson.Parse(legacy, "[null]");
        Equal("[\n  null\n]\n", FerruleJson.Serialize(legacy, retained));
        foreach (var value in new[] { "false", "true" })
        {
            var profile = Json5Descriptor("I");
            var metadata = profile.Replace("\"name\":\"Root\",", "\"name\":\"Root\",\"json_object_item_nullable\":" + value + ",", StringComparison.Ordinal);
            var refused = Json5Stage(Json5Observe("object-item profile " + value, "{n:7}", null, metadata, profile, false), FerruleJson5Stage.Schema);
            if (refused.InnerException is not FerruleJson5SchemaException typed) throw new InvalidOperationException("Object-item profile refusal lost its typed cause.");
            Equal(FerruleJson5SchemaKind.UnsupportedMetadata, typed.Kind);
            Equal("json_object_item_nullable", typed.Field);
        }
        var explicitSchema = ObjectItemSchema(false, true);
        var payload = explicitSchema["FERRULE-EMBEDDED-SCHEMA/4\n".Length..];
        foreach (var value in new[] { "null", "0", "\"true\"" })
        {
            var malformed = payload.Replace("\"json_object_item_nullable\":true", "\"json_object_item_nullable\":" + value, StringComparison.Ordinal);
            try { _ = FerruleJson.Parse(malformed, "[]"); }
            catch (FerruleRuntimeException error)
            {
                Equal(FerruleRuntimeError.JsonBoundary, error.Error);
                Equal("Embedded JSON schema field 'json_object_item_nullable' must be a boolean.", error.Detail);
                continue;
            }
            throw new InvalidOperationException("Malformed item-nullability metadata was accepted.");
        }
        var misplaced = payload.Replace("\"repeating\":true", "\"repeating\":false", StringComparison.Ordinal);
        try { _ = FerruleJson.Parse(misplaced, "{}"); }
        catch (FerruleRuntimeException error)
        {
            Equal(FerruleRuntimeError.JsonBoundary, error.Error);
            Equal("Embedded JSON schema node 'Items' has invalid JSON object-item nullability metadata.", error.Detail);
            return;
        }
        throw new InvalidOperationException("Misplaced item-nullability metadata was accepted.");
    }

    private static void JsonObjectItemPatternSchemaEquality()
    {
        static object ItemSchema(string name, bool nullable) => new
        {
            name, repeating = true, json_object_item_nullable = nullable,
            kind = new { kind = "group", children = Array.Empty<object>() },
        };
        static string Descriptor(bool declaredNullable, bool dynamicNullable) => "FERRULE-EMBEDDED-SCHEMA/4\n" + JsonSerializer.Serialize(new
        {
            name = "Root", json_pattern_property_names = new { sources = new[] { "^A$" } },
            kind = new { kind = "group", children = new[] { ItemSchema("A", declaredNullable) }, @dynamic = ItemSchema("*", dynamicNullable) },
        });
        const string input = "{\"A\":[null]}";
        var originals = new List<(string Schema, string Api, FerruleInstance? Actual, Exception? Error)>();
        foreach (var declared in new[] { false, true })
        {
            var schema = Descriptor(declared, !declared);
            foreach (var api in new[] { "text", "bytes" })
            {
                FerruleInstance? actual = null;
                Exception? error = null;
                try { actual = api == "text" ? FerruleJson.ParseEmbedded(schema, input) : FerruleJson.ParseEmbeddedBytes(schema, Encoding.UTF8.GetBytes(input)); }
                catch (Exception original) { error = original; }
                originals.Add((schema, api, actual, error));
            }
        }
        Console.Error.WriteLine("OBJECT_ITEM_PATTERN_ORIGINALS " + JsonSerializer.Serialize(originals.Select(original => new
        {
            original.Schema, original.Api, actual = JsonRequiredPrecedenceInstance(original.Actual), error = JsonRequiredPrecedenceError(original.Error),
        })));
        Console.Error.Flush();
        const string message = "Embedded JSON schema node 'Root' has declared property 'A' matching a patternProperties selector with a schema that differs from the dynamic schema.";
        foreach (var original in originals)
        {
            Equal<FerruleInstance?>(null, original.Actual);
            if (original.Error is not FerruleRuntimeException error) throw new InvalidOperationException("Incompatible item domains bypassed the pattern-property admission gate.");
            Equal(FerruleRuntimeError.JsonBoundary, error.Error);
            Equal(message, error.Message); Equal(message, error.Detail); Equal<Exception?>(null, error.InnerException);
            foreach (var property in typeof(FerruleRuntimeException).GetProperties(BindingFlags.Instance | BindingFlags.Public | BindingFlags.DeclaredOnly))
                if (property.Name is not (nameof(FerruleRuntimeException.Error) or nameof(FerruleRuntimeException.Detail))) Equal<object?>(null, property.GetValue(error));
        }
        var same = Descriptor(true, true);
        var expected = new FerruleGroup(new[] { new FerruleField("A", new FerruleRepeated(new FerruleInstance[] { new FerruleScalar(FerruleValue.JsonNull) })) });
        Equal(JsonSerializer.Serialize(JsonRequiredPrecedenceInstance(expected)), JsonSerializer.Serialize(JsonRequiredPrecedenceInstance(FerruleJson.ParseEmbedded(same, input))));
        Equal("{\n  \"A\": [\n    null\n  ]\n}\n", FerruleJson.SerializeEmbedded(same, expected));
    }
}
