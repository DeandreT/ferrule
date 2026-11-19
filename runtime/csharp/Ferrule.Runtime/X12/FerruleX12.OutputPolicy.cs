// Selected-only companion. Do not add this file to the legacy X12 source list.
namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    private sealed record SegmentOutputProfile(string Descriptor, Profile Profile, bool CrLf);

    private static readonly global::System.Runtime.CompilerServices.ConditionalWeakTable<
        string,
        SegmentOutputProfile
    > PreparedSegmentOutputProfiles = new();

    private static SegmentOutputProfile PrepareSegmentOutputProfile(string descriptor)
    {
        global::System.ArgumentNullException.ThrowIfNull(descriptor);
        RequireUtf8(descriptor, MaximumSchemaBytes, "X12 descriptor");
        try
        {
            using var parsed = global::System.Text.Json.JsonDocument.Parse(
                descriptor,
                new global::System.Text.Json.JsonDocumentOptions { MaxDepth = 127 }
            );
            var wrapper = parsed.RootElement;
            Fields(
                wrapper,
                [
                    "version",
                    "schema",
                    "separators",
                    "constraints",
                    "lenient_segments",
                    "implied_decimals",
                    "lexical_formats",
                    "autocomplete",
                    "envelope_profile",
                    "segment_line_ending",
                ]
            );
            // Copy raw values, not dictionary entries: nested duplicate keys,
            // member order and their original validation remain observable.
            var buffer = new global::System.Buffers.ArrayBufferWriter<byte>();
            using (var writer = new global::System.Text.Json.Utf8JsonWriter(buffer))
            {
                writer.WriteStartObject();
                foreach (var property in wrapper.EnumerateObject())
                {
                    if (property.NameEquals("segment_line_ending"))
                        continue;
                    writer.WritePropertyName(property.Name);
                    writer.WriteRawValue(property.Value.GetRawText(), skipInputValidation: true);
                }
                writer.WriteEndObject();
            }
            string legacyDescriptor = StrictUtf8.GetString(buffer.WrittenSpan);
            Profile profile = PreparedProfile(legacyDescriptor, input: false);
            // Old schema/options validation precedes the selected-only value.
            bool crlf = wrapper.TryGetProperty("segment_line_ending", out var selected)
                ? selected.GetString() switch
                {
                    "lf" => false,
                    "crlf" => true,
                    _ => throw Failure(
                        FerruleX12Error.Schema,
                        "Unsupported X12 segment line ending."
                    ),
                }
                : false;
            if (crlf && profile.OutputSyntax.Segment is '\n' or '\r')
                throw Failure(
                    FerruleX12Error.Schema,
                    "CRLF formatting requires a visible segment terminator."
                );
            return new(legacyDescriptor, profile, crlf);
        }
        catch (FerruleX12Exception)
        {
            throw;
        }
        catch (global::System.Exception error)
            when (error
                    is global::System.Text.Json.JsonException
                        or global::System.InvalidOperationException
                        or global::System.FormatException
            )
        {
            throw Failure(FerruleX12Error.Schema, "Invalid embedded X12 output policy.");
        }
    }

    internal static string SerializeEmbeddedWithLineEnding(
        string descriptor,
        FerruleInstance document
    ) => SerializeSegmentOutput(descriptor, document, null).Text.ToString();

    internal static string SerializeEmbeddedWithLineEnding(
        string descriptor,
        FerruleInstance document,
        FerruleExecutionContext executionContext
    )
    {
        global::System.ArgumentNullException.ThrowIfNull(executionContext);
        return SerializeSegmentOutput(descriptor, document, executionContext).Text.ToString();
    }

    internal static byte[] SerializeEmbeddedBytesWithLineEnding(
        string descriptor,
        FerruleInstance document
    ) => EncodeOutput(SerializeSegmentOutput(descriptor, document, null));

    internal static byte[] SerializeEmbeddedBytesWithLineEnding(
        string descriptor,
        FerruleInstance document,
        FerruleExecutionContext executionContext
    )
    {
        global::System.ArgumentNullException.ThrowIfNull(executionContext);
        return EncodeOutput(SerializeSegmentOutput(descriptor, document, executionContext));
    }

    private static CompletedOutput SerializeSegmentOutput(
        string descriptor,
        FerruleInstance document,
        FerruleExecutionContext? executionContext
    )
    {
        global::System.ArgumentNullException.ThrowIfNull(document);
        global::System.ArgumentNullException.ThrowIfNull(descriptor);
        var selected = PreparedSegmentOutputProfiles.GetValue(
            descriptor,
            PrepareSegmentOutputProfile
        );
        CompletedOutput output = SerializeOutput(selected.Descriptor, document, executionContext);
        return selected.CrLf ? ExpandSegmentLineEndings(output, MaximumDocumentBytes) : output;
    }

    // Only the strict canonical writer's completed builder reaches this method.
    // It rejects field control text and the profile has a visible terminator,
    // so every LF is exactly one physical segment suffix (including trailers).
    private static CompletedOutput ExpandSegmentLineEndings(CompletedOutput output, long maximum)
    {
        long bytes = output.Bytes;
        foreach (var chunk in output.Text.GetChunks())
        foreach (char character in chunk.Span)
            if (character == '\n')
            {
                if (bytes >= maximum)
                    throw Limit("X12 output byte limit exceeded.");
                bytes++;
            }
        if (bytes > maximum)
            throw Limit("X12 output byte limit exceeded.");
        // No expansion/allocation until the exact completed wire length passes.
        output.Text.Replace("\n", "\r\n");
        return new(output.Text, bytes);
    }
}
