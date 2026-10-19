using System.Collections.ObjectModel;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public enum FerruleJsonSourceKind { Null, Boolean, Number, String, Array, Object }
public enum FerruleJsonSourceErrorKind
{
    InvalidLimits, InvalidIdentity, DocumentBytes, Utf8, Syntax, ContainerDepth, Values,
}

public sealed class FerruleJsonSourceException : Exception
{
    public FerruleJsonSourceErrorKind Kind { get; }

    internal FerruleJsonSourceException(FerruleJsonSourceErrorKind kind, Exception? cause = null)
        : base($"JSON source refused: {kind}", cause) => Kind = kind;
}

/// <summary>Per-document limits, each at most its hard maximum. Zero is allowed.</summary>
public sealed record FerruleJsonSourceLimits
{
    public int DocumentBytes { get; init; } = FerruleJsonSourceDocument.MaxDocumentBytes;
    public int ContainerDepth { get; init; } = FerruleJsonSourceDocument.MaxContainerDepth;
    public int Values { get; init; } = FerruleJsonSourceDocument.MaxValues;
}

/// <summary>A parsed value with immutable ordered members and lexical numbers.</summary>
public sealed class FerruleJsonSourceValue
{
    private readonly string? text;
    private readonly bool? boolean;
    public FerruleJsonSourceKind Kind { get; }
    public IReadOnlyList<FerruleJsonSourceValue> Items { get; }
    public IReadOnlyList<KeyValuePair<string, FerruleJsonSourceValue>> Members { get; }

    internal FerruleJsonSourceValue(FerruleJsonSourceKind kind, string? text = null, bool? boolean = null,
        List<FerruleJsonSourceValue>? items = null,
        List<KeyValuePair<string, FerruleJsonSourceValue>>? members = null)
    {
        Kind = kind;
        this.text = text;
        this.boolean = boolean;
        Items = new ReadOnlyCollection<FerruleJsonSourceValue>(items ?? []);
        Members = new ReadOnlyCollection<KeyValuePair<string, FerruleJsonSourceValue>>(members ?? []);
    }

    public string? StringValue => Kind == FerruleJsonSourceKind.String ? text : null;
    public string? NumberText => Kind == FerruleJsonSourceKind.Number ? text : null;
    public bool? BooleanValue => boolean;

    /// <summary>Every exact decoded-name match, in source order, without coercion.</summary>
    public IEnumerable<FerruleJsonSourceValue> MembersNamed(string name)
    {
        ArgumentNullException.ThrowIfNull(name);
        foreach (var member in Members)
            if (string.Equals(member.Key, name, StringComparison.Ordinal))
                yield return member.Value;
    }
}

/// <summary>
/// Schema-independent JSON parsing. Identity is host-supplied metadata; this type
/// performs no transport, schema validation, numeric conversion or mapping.
/// </summary>
public sealed class FerruleJsonSourceDocument
{
    public const int MaxDocumentBytes = 64 * 1024 * 1024;
    public const int MaxContainerDepth = 64;
    public const int MaxValues = 1_000_000;
    public const int MaxIdentityBytes = 4096;
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    public string Identity { get; }
    public FerruleJsonSourceValue Root { get; }

    private FerruleJsonSourceDocument(string identity, FerruleJsonSourceValue root)
    {
        Identity = identity;
        Root = root;
    }

    public static FerruleJsonSourceDocument Parse(string identity, ReadOnlySpan<byte> bytes,
        FerruleJsonSourceLimits? limits = null)
    {
        limits ??= new();
        if (limits.DocumentBytes is < 0 or > MaxDocumentBytes ||
            limits.ContainerDepth is < 0 or > MaxContainerDepth || limits.Values is < 0 or > MaxValues)
            throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.InvalidLimits);
        try
        {
            if (string.IsNullOrEmpty(identity) || identity.Contains('\0') ||
                StrictUtf8.GetByteCount(identity) > MaxIdentityBytes)
                throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.InvalidIdentity);
        }
        catch (EncoderFallbackException error)
        {
            throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.InvalidIdentity, error);
        }
        if (bytes.Length > limits.DocumentBytes)
            throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.DocumentBytes);
        try { _ = StrictUtf8.GetCharCount(bytes); }
        catch (DecoderFallbackException error)
        {
            throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.Utf8, error);
        }
        if (bytes.StartsWith(new byte[] { 0xef, 0xbb, 0xbf })) bytes = bytes[3..];
        CheckDepth(bytes, limits.ContainerDepth);
        try
        {
            var reader = new Utf8JsonReader(bytes, new JsonReaderOptions
            {
                MaxDepth = MaxContainerDepth,
                CommentHandling = JsonCommentHandling.Disallow,
                AllowTrailingCommas = false,
            });
            if (!reader.Read()) throw new JsonException("Expected one JSON value.");
            var remaining = limits.Values;
            var root = ReadValue(ref reader, ref remaining);
            if (reader.Read()) throw new JsonException("Unexpected trailing JSON value.");
            return new(identity, root);
        }
        catch (JsonException error)
        {
            throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.Syntax, error);
        }
        catch (InvalidOperationException error)
        {
            // GetString rejects malformed escaped surrogate pairs as well as
            // the byte decoder rejecting malformed UTF-8 above.
            throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.Syntax, error);
        }
    }

    private static FerruleJsonSourceValue ReadValue(ref Utf8JsonReader reader, ref int remaining)
    {
        if (remaining == 0) throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.Values);
        remaining--;
        switch (reader.TokenType)
        {
            case JsonTokenType.Null: return new(FerruleJsonSourceKind.Null);
            case JsonTokenType.True: return new(FerruleJsonSourceKind.Boolean, boolean: true);
            case JsonTokenType.False: return new(FerruleJsonSourceKind.Boolean, boolean: false);
            case JsonTokenType.String: return new(FerruleJsonSourceKind.String, text: reader.GetString());
            case JsonTokenType.Number:
                return new(FerruleJsonSourceKind.Number, text: Encoding.UTF8.GetString(reader.ValueSpan));
            case JsonTokenType.StartArray:
                var items = new List<FerruleJsonSourceValue>();
                while (reader.Read())
                {
                    if (reader.TokenType == JsonTokenType.EndArray)
                        return new(FerruleJsonSourceKind.Array, items: items);
                    items.Add(ReadValue(ref reader, ref remaining));
                }
                throw new JsonException("Unterminated array.");
            case JsonTokenType.StartObject:
                var members = new List<KeyValuePair<string, FerruleJsonSourceValue>>();
                while (reader.Read())
                {
                    if (reader.TokenType == JsonTokenType.EndObject)
                        return new(FerruleJsonSourceKind.Object, members: members);
                    if (reader.TokenType != JsonTokenType.PropertyName)
                        throw new JsonException("Expected a member name.");
                    var name = reader.GetString()!;
                    if (!reader.Read()) throw new JsonException("Expected a member value.");
                    members.Add(new(name, ReadValue(ref reader, ref remaining)));
                }
                throw new JsonException("Unterminated object.");
            default: throw new JsonException("Expected a JSON value.");
        }
    }

    // This admission scan shields strings/escapes; Utf8JsonReader owns grammar.
    private static void CheckDepth(ReadOnlySpan<byte> bytes, int maximum)
    {
        var depth = 0;
        var quoted = false;
        var escaped = false;
        foreach (var value in bytes)
        {
            if (quoted)
            {
                if (escaped) escaped = false;
                else if (value == '\\') escaped = true;
                else if (value == '"') quoted = false;
            }
            else if (value == '"') quoted = true;
            else if (value is (byte)'{' or (byte)'[')
            {
                if (++depth > maximum)
                    throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.ContainerDepth);
            }
            else if (value is (byte)'}' or (byte)']') depth = Math.Max(0, depth - 1);
        }
    }
}
