using System.Buffers;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    /// <summary>Serialize a mapped document with reference-style JSON5 and exact decimal values.
    /// Schema metadata is retained but no second required/range/default validation is imposed.</summary>
    public static string SerializeMapForceJson5AsciiKeys(string descriptor, FerruleInstance instance, int maximumBytes = 64 * 1024 * 1024)
    {
        ArgumentNullException.ThrowIfNull(descriptor);
        ArgumentNullException.ThrowIfNull(instance);
        if (maximumBytes is < 0 or > 64 * 1024 * 1024)
            throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.InvalidLimit, "JSON5 output limit exceeds the hard maximum.");
        JsonSchemaNode schema;
        try { schema = ParseSchema(descriptor, true); }
        catch (Exception error) when (error is FerruleRuntimeException or JsonException)
        { throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.Schema, "Invalid mapped JSON schema.", error); }
        ValidateMapForceSchema(schema, 0);
        var output = new FerruleMapForceJson5.Output(maximumBytes);
        int remaining = 1_000_000;
        WriteMapForceInstance(output, schema, instance, 0, false, ref remaining);
        return output.ToString();
    }

    private static void WriteMapForceInstance(FerruleMapForceJson5.Output output,
        JsonSchemaNode schema, FerruleInstance instance, int depth, bool item, ref int remaining)
    {
        if (remaining-- == 0) throw MapForceShape("Value limit exceeded.");
        if (depth > 64 || depth == 64 && instance is FerruleGroup or FerruleRepeated)
            throw MapForceShape("Container depth exceeded.");
        if (instance is FerruleScalar { Value.Kind: FerruleValueKind.JsonNull })
        {
            output.Add("null");
            return;
        }
        if (schema.Repeating && !item)
        {
            if (instance is not FerruleRepeated repeated) throw MapForceShape("Expected a complete repeated value.");
            output.Add("[");
            int index = 0;
            foreach (var value in repeated.Items)
            {
                if (index++ > 0) output.Add(",");
                output.Line(depth + 1);
                WriteMapForceInstance(output, schema, value, depth + 1, true, ref remaining);
            }
            output.Line(depth);
            output.Add("]");
            return;
        }
        if (schema.JsonAny)
        {
            if (instance is not FerruleScalar scalar) throw MapForceShape("json_any requires a scalar representation.");
            if (scalar.Value.Kind == FerruleValueKind.String)
            {
                try
                {
                    string text = scalar.Value.StringValue;
                    if (StrictUtf8.GetByteCount(text) > 64 * 1024 * 1024)
                        throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.Input, "Encoded JSON exceeds the byte limit.");
                    if (text.StartsWith('\uFEFF')) text = text[1..];
                    using var parsed = JsonDocument.Parse(text, new JsonDocumentOptions { MaxDepth = 64 });
                    remaining++;
                    CheckMapForceJsonValue(parsed.RootElement, depth, ref remaining);
                    output.Value(parsed.RootElement, depth);
                }
                catch (JsonException error)
                { throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.Input, "Invalid encoded JSON value.", error); }
            }
            else WriteMapForceScalar(output, scalar.Value, null);
            return;
        }
        if (schema.ScalarDomain == JsonScalarDomain.None && instance is FerruleGroup group)
        {
            output.Add("{");
            bool first = true;
            foreach (var field in group.Fields)
            {
                if (field.Value is FerruleScalar { Value.Kind: FerruleValueKind.Null })
                {
                    if (remaining-- == 0) throw MapForceShape("Value limit exceeded.");
                    continue;
                }
                var member = schema.Children.FirstOrDefault(child => child.Name == field.Name) ?? schema.Dynamic;
                if (member is null) throw MapForceShape("Member is outside a closed schema.");
                if (!first) output.Add(",");
                first = false;
                output.Line(depth + 1);
                output.Key(field.Name);
                output.Add(": ");
                WriteMapForceInstance(output, member, field.Value, depth + 1, false, ref remaining);
            }
            if (!first) output.Line(depth);
            output.Add("}");
            return;
        }
        if (instance is FerruleScalar valueScalar)
        {
            WriteMapForceScalar(output, valueScalar.Value, schema.ScalarDomain);
            return;
        }
        throw MapForceShape("Mapped value does not match its scalar or object shape.");
    }

    private static void CheckMapForceJsonValue(JsonElement value, int depth, ref int remaining)
    {
        if (remaining-- == 0) throw MapForceShape("Value limit exceeded.");
        if (depth >= 64 && value.ValueKind is JsonValueKind.Array or JsonValueKind.Object)
            throw MapForceShape("Container depth exceeded.");
        if (value.ValueKind == JsonValueKind.Array)
            foreach (var item in value.EnumerateArray()) CheckMapForceJsonValue(item, depth + 1, ref remaining);
        else if (value.ValueKind == JsonValueKind.Object)
            foreach (var field in value.EnumerateObject()) CheckMapForceJsonValue(field.Value, depth + 1, ref remaining);
    }

    private static void WriteMapForceScalar(FerruleMapForceJson5.Output output, FerruleValue value, JsonScalarDomain? expected)
    {
        switch (value.Kind)
        {
            case FerruleValueKind.String when expected is null or JsonScalarDomain.String:
                output.Quote(value.StringValue);
                return;
            case FerruleValueKind.Bool when expected is null or JsonScalarDomain.Bool:
                output.Add(value.BooleanValue ? "true" : "false");
                return;
            case FerruleValueKind.Int64 when expected is null or JsonScalarDomain.Int64 or JsonScalarDomain.Double:
                output.NumberText(value.Int64Value.ToString(System.Globalization.CultureInfo.InvariantCulture));
                return;
            case FerruleValueKind.Decimal when expected is null or JsonScalarDomain.Double:
                output.NumberText(value.DecimalValue.ToString());
                return;
            case FerruleValueKind.Double when expected is null or JsonScalarDomain.Double:
                try { output.NumberText(FerruleDecimal96.FromDouble(value.DoubleValue).ToString()); }
                catch (FerruleDecimalException error)
                { throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.Number, "Invalid decimal conversion.", error); }
                return;
            case FerruleValueKind.JsonNull:
                output.Add("null");
                return;
            default: throw MapForceShape("Mapped scalar kind is not admitted for this member.");
        }
    }

    private static FerruleMapForceJson5Exception MapForceShape(string message) =>
        new(FerruleMapForceJson5Error.Shape, message);

    private static void ValidateMapForceSchema(JsonSchemaNode schema, int depth)
    {
        if (depth > 64) throw MapForceShape("Schema depth exceeds the mapped JSON5 profile.");
        if (schema.RecursiveReference is not null || schema.JsonArrayWrapper)
            throw MapForceShape("Recursive and nested-array schemas require separate admission.");
        if (schema.ScalarDomain == JsonScalarDomain.None)
        {
            if (schema.JsonAny || schema.Alternatives.Count != 0)
                throw MapForceShape("Object alternatives require separate admission.");
            var names = new HashSet<string>(StringComparer.Ordinal);
            foreach (var child in schema.Children)
            {
                if (child.Name.Any(c => c > 127) || !names.Add(child.Name))
                    throw MapForceShape("Declared member names must be unique ASCII strings.");
                ValidateMapForceSchema(child, depth + 1);
            }
            if (schema.Dynamic is not null) ValidateMapForceSchema(schema.Dynamic, depth + 1);
        }
        else
        {
            if (schema.ScalarDomain is not (JsonScalarDomain.String or JsonScalarDomain.Int64 or JsonScalarDomain.Double or JsonScalarDomain.Bool))
                throw MapForceShape("Scalar unions require separate admission.");
            if (schema.JsonAny && schema.ScalarDomain != JsonScalarDomain.String)
                throw MapForceShape("json_any requires its encoded String representation.");
        }
    }

    internal static string ExecuteJson5Accepted(
        string sourceDescriptor, string targetDescriptor, string source,
        Func<FerruleInstance, FerruleInstance> mapping)
    {
        var inputSchema = Json5Schema(sourceDescriptor, FerruleJson5SchemaSide.Source);
        var outputSchema = Json5Schema(targetDescriptor, FerruleJson5SchemaSide.Target);
        string normalized;
        try { normalized = FerruleJson5Syntax.NormalizeBoundaryText(source); }
        catch (FerruleJson5SyntaxException error)
        { throw new FerruleJson5BoundaryException(FerruleJson5Stage.Syntax, error); }
        FerruleInstance input;
        try
        {
            using var parsed = JsonDocument.Parse(normalized, new JsonDocumentOptions
            { MaxDepth = MaximumParsedDepth, CommentHandling = JsonCommentHandling.Disallow });
            if (parsed.RootElement.ValueKind != JsonValueKind.Object)
            { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.RootObjectRequired); }
            ValidateInputJsonScalars(inputSchema, parsed.RootElement);
            input = ReadNode(inputSchema, parsed.RootElement, new NodeBudget(), 0);
        }
        catch (Exception error)
        { throw new FerruleJson5BoundaryException(FerruleJson5Stage.Input, error); }
        FerruleInstance output;
        try { output = mapping(input); }
        catch (Exception error)
        { throw new FerruleJson5BoundaryException(FerruleJson5Stage.Mapping, error); }
        try { return Json5Serialize(outputSchema, output); }
        catch (Exception error)
        { throw new FerruleJson5BoundaryException(FerruleJson5Stage.Output, error); }
    }

    private static JsonSchemaNode Json5Schema(string descriptor, FerruleJson5SchemaSide side)
    {
        try
        {
            var admitted = ValidateJson5Descriptor(descriptor);
            return ParseSchema(admitted, true);
        }
        catch (Exception error)
        { throw new FerruleJson5BoundaryException(FerruleJson5Stage.Schema, error, side); }
    }

    private static string Json5ReaderDescriptor(JsonElement root, bool versioned)
    {
        // Complete original admission precedes this optional-only projection.
        // The compact strings use the unchanged canonical JSON escaping seam:
        // no pretty indentation or unnecessary Unicode escaping can expand the
        // admitted descriptor before the ordinary reader repeats its byte cap.
        var text = new System.Text.StringBuilder();
        Json5ReaderValue(text, root);
        var payload = text.ToString();
        return versioned ? FerruleEmbeddedSchema.V2Prefix + payload : payload;
    }

    private static void Json5ReaderValue(System.Text.StringBuilder text, JsonElement value)
    {
        if (value.ValueKind == JsonValueKind.Object)
        {
            text.Append('{');
            var first = true;
            foreach (var field in value.EnumerateObject())
            {
                // Only default-valued null metadata can reach this projection.
                if (field.Value.ValueKind == JsonValueKind.Null) { continue; }
                if (!first) { text.Append(','); }
                first = false;
                AppendSerdeString(text, field.Name);
                text.Append(':');
                Json5ReaderValue(text, field.Value);
            }
            text.Append('}');
        }
        else if (value.ValueKind == JsonValueKind.Array)
        {
            text.Append('[');
            var first = true;
            foreach (var item in value.EnumerateArray())
            {
                if (!first) { text.Append(','); }
                first = false;
                Json5ReaderValue(text, item);
            }
            text.Append(']');
        }
        else { AppendCanonicalAnyJson(text, value, preserveNumbers: true); }
    }

    private static string Json5Serialize(JsonSchemaNode schema, FerruleInstance instance)
    {
        ArgumentNullException.ThrowIfNull(instance);
        try
        {
            // Same strict writer/number canonicalization as SerializeEmbedded;
            // profile forbids its repeated-root-array special case.
            var buffer = new ArrayBufferWriter<byte>();
            using (var writer = new Utf8JsonWriter(buffer, new JsonWriterOptions
            {
                Indented = true, Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
                MaxDepth = MaximumDepth, SkipValidation = false,
            })) { WriteNode(writer, schema, instance, new NodeBudget(), 0); }
            var text = CanonicalizeOutputText(buffer.WrittenSpan);
            text.Append('\n');
            var canonical = text.ToString();
            var bytes = FerruleJson5Syntax.CountBoundaryBytes(canonical);
            if (bytes > MaximumDocumentBytes)
            {
                throw new FerruleJson5ResourceException(FerruleJson5Resource.OutputDocumentBytes,
                    bytes, MaximumDocumentBytes);
            }
            return canonical;
        }
        catch (FerruleRuntimeException) { throw; }
        catch (Exception error) when (error is JsonException or FormatException or InvalidOperationException or OverflowException or System.Text.EncoderFallbackException)
        { throw Boundary("JSON output is invalid.", error); }
    }
}
