using System.Buffers;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
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
            var canonical = CanonicalizeOutputStrings(buffer.WrittenSpan);
            var bytes = checked(FerruleJson5Syntax.CountBoundaryBytes(canonical) + 1);
            if (bytes > MaximumDocumentBytes)
            {
                throw new FerruleJson5ResourceException(FerruleJson5Resource.OutputDocumentBytes,
                    bytes, MaximumDocumentBytes);
            }
            return canonical + "\n";
        }
        catch (FerruleRuntimeException) { throw; }
        catch (Exception error) when (error is JsonException or FormatException or InvalidOperationException or OverflowException or System.Text.EncoderFallbackException)
        { throw Boundary("JSON output is invalid.", error); }
    }
}
