using System.Buffers;
using System.Runtime.CompilerServices;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    internal static string ExecuteJson5Accepted(
        string sourceDescriptor, string targetDescriptor, string source,
        Func<FerruleInstance, FerruleInstance> mapping) =>
        ExecuteJson5Accepted(sourceDescriptor, targetDescriptor, source, mapping,
            static output => output.Text.ToString());

    internal static byte[] ExecuteJson5AcceptedUtf8(
        string sourceDescriptor, string targetDescriptor, string source,
        Func<FerruleInstance, FerruleInstance> mapping) =>
        ExecuteJson5Accepted(sourceDescriptor, targetDescriptor, source, mapping, Json5Utf8);

    private static T ExecuteJson5Accepted<T>(
        string sourceDescriptor, string targetDescriptor, string source,
        Func<FerruleInstance, FerruleInstance> mapping, Func<Json5Output, T> complete)
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
        try { return complete(Json5Serialize(outputSchema, output)); }
        catch (Exception error)
        { throw new FerruleJson5BoundaryException(FerruleJson5Stage.Output, error); }
    }

    // Generated descriptor constants are interned, so each JSON5 descriptor is
    // admitted and parsed once per process and shared by both sides of
    // repeated and concurrent calls. The table compares keys by reference and
    // does not keep them alive. Only successful preparations are retained; a
    // rejected descriptor is admitted again by every call and throws its
    // original error for its own side. Every call keeps its own normalization,
    // NodeBudget and output accounting.
    private static readonly ConditionalWeakTable<string, JsonSchemaNode> PreparedJson5Schemas = new();

    private static JsonSchemaNode Json5Schema(string descriptor, FerruleJson5SchemaSide side)
    {
        if (PreparedJson5Schemas.TryGetValue(descriptor, out var prepared)) { return prepared; }
        JsonSchemaNode parsed;
        try
        {
            var admitted = ValidateJson5Descriptor(descriptor);
            parsed = ParseSchema(admitted, true);
        }
        catch (Exception error)
        { throw new FerruleJson5BoundaryException(FerruleJson5Stage.Schema, error, side); }
        // A concurrent first call may have prepared the same descriptor; keep
        // the first published graph.
        return PreparedJson5Schemas.GetValue(descriptor, _ => parsed);
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

    /// <summary>Complete canonical output with its final newline and counted UTF-8 length.</summary>
    private readonly record struct Json5Output(System.Text.StringBuilder Text, long Bytes);

    private static readonly System.Text.UTF8Encoding Json5StrictUtf8 = new(false, true);

    /// <summary>
    /// Counts the canonical builder's UTF-8 bytes without materializing it. The
    /// canonical text is decoded from the writer's UTF-8 and so is well-formed;
    /// should that ever fail, the original boundary census reports it.
    /// </summary>
    private static long Json5OutputBytes(System.Text.StringBuilder text)
    {
        var encoder = Json5StrictUtf8.GetEncoder();
        long bytes = 0;
        try
        {
            foreach (var chunk in text.GetChunks()) { bytes += encoder.GetByteCount(chunk.Span, flush: false); }
            return bytes + encoder.GetByteCount(ReadOnlySpan<char>.Empty, flush: true);
        }
        catch (System.Text.EncoderFallbackException)
        { return FerruleJson5Syntax.CountBoundaryBytes(text.ToString()); }
    }

    /// <summary>
    /// Encodes the counted output straight from its builder into one array, so
    /// byte routes never materialize the UTF-16 document.
    /// </summary>
    private static byte[] Json5Utf8(Json5Output output)
    {
        var bytes = new byte[output.Bytes];
        var encoder = Json5StrictUtf8.GetEncoder();
        var written = 0;
        foreach (var chunk in output.Text.GetChunks())
        { written += encoder.GetBytes(chunk.Span, bytes.AsSpan(written), flush: false); }
        written += encoder.GetBytes(ReadOnlySpan<char>.Empty, bytes.AsSpan(written), flush: true);
        if (written != bytes.Length)
        { throw new InvalidOperationException("JSON5 output encoding changed its counted length."); }
        return bytes;
    }

    private static Json5Output Json5Serialize(JsonSchemaNode schema, FerruleInstance instance)
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
            // The limit applies before either route materializes the document.
            var bytes = Json5OutputBytes(text);
            if (bytes > MaximumDocumentBytes)
            {
                throw new FerruleJson5ResourceException(FerruleJson5Resource.OutputDocumentBytes,
                    bytes, MaximumDocumentBytes);
            }
            return new Json5Output(text, bytes);
        }
        catch (FerruleRuntimeException) { throw; }
        catch (Exception error) when (error is JsonException or FormatException or InvalidOperationException or OverflowException or System.Text.EncoderFallbackException)
        { throw Boundary("JSON output is invalid.", error); }
    }
}
