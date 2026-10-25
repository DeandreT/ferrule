using System.Globalization;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public enum FerruleEdiError { Descriptor, Format, Schema, Value, Constraint, Limit, FieldWidth, Envelope }
public sealed class FerruleEdiException(FerruleEdiError code, string message, long? bytes = null, long? maximum = null, string? field = null, int? segmentIndex = null) : Exception(message)
{
    public FerruleEdiError Code { get; } = code;
    public long? Bytes { get; } = bytes;
    public long? Maximum { get; } = maximum;
    public string? Field { get; } = field;
    public int? SegmentIndex { get; } = segmentIndex;
}

public sealed class FerruleEdiInputBudget
{
    private long total;
    public void Charge(int bytes)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(bytes);
        const long document = 64L * 1024 * 1024, maximum = 256L * 1024 * 1024;
        if (bytes > document) throw new FerruleEdiException(FerruleEdiError.Limit, "EDI document exceeds its byte limit.", bytes, document);
        long next = total + bytes;
        if (next > maximum) throw new FerruleEdiException(FerruleEdiError.Limit, "EDI run exceeds its byte limit.", next, maximum);
        total = next;
    }
}

/// <summary>Package-free, schema-guided EDI document boundary. A prepared boundary owns no host.</summary>
public sealed partial class FerruleEdi
{
    private const int MaximumBytes = 64 * 1024 * 1024;
    private static readonly UTF8Encoding Utf8 = new(false, true);
    private sealed record Node(string Name, string? Type, Node[] Children, bool Repeating, string? Fixed);
    private sealed record Segment(string Id, List<List<List<string>>> Elements);
    private sealed record Syntax(char Element, char Component, char Terminator, char? Release, char? Repetition, char? Subcomponent = null, string? ReadComponent = null);
    private readonly Node _root;
    private readonly string _kind;
    private readonly JsonElement _options;
    private readonly bool _lenient;
    private readonly bool _strictEnvelope;
    private readonly Dictionary<string, JsonElement> _layouts = new(StringComparer.Ordinal);
    private static FerruleEdiException Fail(FerruleEdiError code, string text) => new(code, text);
    private static string? Text(JsonElement value, string name) => value.TryGetProperty(name, out var found) && found.ValueKind == JsonValueKind.String ? found.GetString() : null;
    private static bool Flag(JsonElement value, string name) => value.TryGetProperty(name, out var found) && found.ValueKind == JsonValueKind.True;
    private static JsonElement[] Array(JsonElement value, string name) => value.TryGetProperty(name, out var found) && found.ValueKind == JsonValueKind.Array ? found.EnumerateArray().ToArray() : [];
    private static FerruleInstance? Field(FerruleInstance? value, string name) => value is FerruleGroup group ? group.Fields.FirstOrDefault(field => field.Name == name)?.Value : null;

    public FerruleEdi(string schema, string options, string kind, bool input = false, bool strictEnvelope = false)
    {
        if (Utf8.GetByteCount(schema) > 1024 * 1024 || Utf8.GetByteCount(options) > 1024 * 1024)
            throw Fail(FerruleEdiError.Limit, "EDI descriptor exceeds 1 MiB.");
        if (kind is not ("X12" or "Edifact" or "Hl7" or "Tradacoms" or "Idoc"))
            throw Fail(FerruleEdiError.Descriptor, "Unsupported EDI family.");
        _kind = kind;
        if (strictEnvelope && kind != "Edifact") throw Fail(FerruleEdiError.Descriptor, "Strict envelope validation is available for EDIFACT.");
        _strictEnvelope = strictEnvelope;
        try
        {
            using var parsedSchema = JsonDocument.Parse(schema, new JsonDocumentOptions { MaxDepth = 256 });
            int count = 0;
            _root = ParseNode(parsedSchema.RootElement, 0, ref count);
            if (_root.Repeating || _root.Type is not null) throw Fail(FerruleEdiError.Descriptor, "EDI documents require a singular root container.");
            if (_root.Name is "MFD-X12" or "MFD-EDIFACT") throw Fail(FerruleEdiError.Descriptor, "An unresolved EDI entry tree has no reliable element positions.");
            using var parsedOptions = JsonDocument.Parse(options, new JsonDocumentOptions { MaxDepth = 128 });
            _options = parsedOptions.RootElement.Clone();
            _lenient = Flag(_options, "lenient_segments");
            foreach (string option in new[] { "edi_implied_decimals", "edi_lexical_formats", "edi_value_constraints" })
            {
                var seen = new HashSet<string>(StringComparer.Ordinal);
                foreach (var rule in Array(_options, option))
                {
                    var path = Array(rule, "path").Select(part => part.GetString()!).ToArray();
                    if (path.Length is 0 or > 64 || !seen.Add(string.Join('\0', path))) throw Fail(FerruleEdiError.Descriptor, "Duplicate or overlong EDI option path.");
                    Node leaf = _root;
                    foreach (string part in path) leaf = leaf.Children.FirstOrDefault(child => child.Name == part) ?? throw Fail(FerruleEdiError.Descriptor, "EDI option path is absent from schema.");
                    if (leaf.Type is null || input && option == "edi_implied_decimals" && leaf.Type != "float") throw Fail(FerruleEdiError.Descriptor, "Invalid EDI option target.");
                }
            }
            if (_options.TryGetProperty("edi_config_reference", out var unresolved) && unresolved.ValueKind != JsonValueKind.Null)
                throw Fail(FerruleEdiError.Descriptor, "Unresolved EDI configuration.");
            if (kind == "Idoc")
            {
                if (!_options.TryGetProperty("idoc", out var layout)) throw Fail(FerruleEdiError.Descriptor, "Missing IDoc layout.");
                foreach (var segment in Array(layout, "segments")) _layouts.Add(Text(segment, "name")!, segment);
                ValidateLayout(_root, true);
            }
            else if (input && !_lenient)
            {
                bool valid = Triggers(_root, true).Select(node => Id(node.Name)).Any(id => kind switch { "X12" => id == "ISA", "Edifact" => id is "UNB" or "UNH", "Hl7" => id is "FHS" or "BHS" or "MSH", _ => id == "STX" });
                if (!valid) throw Fail(FerruleEdiError.Descriptor, "Strict EDI input schema has no matching opening segment.");
            }
        }
        catch (JsonException error) { throw Fail(FerruleEdiError.Descriptor, error.Message); }
    }

    private static Node ParseNode(JsonElement value, int depth, ref int count)
    {
        if (depth > 64 || ++count > 10_000) throw Fail(FerruleEdiError.Limit, "EDI schema limit exceeded.");
        string name = Text(value, "name") ?? throw Fail(FerruleEdiError.Descriptor, "Missing schema name.");
        var kind = value.GetProperty("kind");
        string? type = Text(kind, "kind") == "scalar" ? Text(kind, "ty") : null;
        if (type is not null && type is not ("string" or "int" or "float" or "bool")) throw Fail(FerruleEdiError.Descriptor, "Invalid scalar type.");
        if (type is null && Text(kind, "kind") != "group") throw Fail(FerruleEdiError.Descriptor, "EDI requires scalar or group schema nodes.");
        var children = new List<Node>();
        foreach (var child in Array(kind, "children")) children.Add(ParseNode(child, depth + 1, ref count));
        if (children.Select(child => child.Name).Distinct(StringComparer.Ordinal).Count() != children.Count)
            throw Fail(FerruleEdiError.Descriptor, "Duplicate EDI field name.");
        return new(name, type, children.ToArray(), Flag(value, "repeating"), Text(value, "fixed"));
    }

    private void ValidateLayout(Node node, bool root)
    {
        if (!root && _layouts.TryGetValue(node.Name, out var layout))
        {
            var fields = Array(layout, "fields");
            if (node.Children.Length != fields.Length || node.Children.Where((child, at) => child.Type is null || child.Repeating || child.Name != Text(fields[at], "name")).Any())
                throw Fail(FerruleEdiError.Descriptor, "IDoc layout does not match its positional schema.");
            var ranges = fields.Select(field => (First: field.GetProperty("first_byte").GetInt32(), Last: field.GetProperty("last_byte").GetInt32())).Append((First: 1, Last: Utf8.GetByteCount(node.Name))).OrderBy(range => range.First).ToArray();
            if (ranges.Any(range => range.First < 1 || range.Last < range.First || range.Last > 1024 * 1024) || ranges.Skip(1).Where((range, at) => range.First <= ranges[at].Last).Any())
                throw Fail(FerruleEdiError.Descriptor, "Overlapping or invalid IDoc field layout.");
        }
        else
        {
            if (node.Type is not null) throw Fail(FerruleEdiError.Descriptor, "IDoc containers must be groups.");
            foreach (var child in node.Children) ValidateLayout(child, false);
        }
    }

    private static string? Id(string name)
    {
        string candidate = name.StartsWith("MF_", StringComparison.Ordinal) ? name[3..] : name;
        int suffix = candidate.LastIndexOf('_');
        if (suffix >= 0 && suffix + 1 < candidate.Length && candidate[(suffix + 1)..].All(char.IsAsciiDigit)) candidate = candidate[..suffix];
        if (candidate.StartsWith("SG", StringComparison.Ordinal) && candidate.Length > 2 && candidate[2..].All(char.IsAsciiDigit)) return null;
        return candidate.Length is >= 2 and <= 30 && char.IsAsciiLetterUpper(candidate[0]) && candidate.All(c => char.IsAsciiLetterUpper(c) || char.IsAsciiDigit(c)) ? candidate : null;
    }

    private IEnumerable<Node> Triggers(Node node, bool root = false)
    {
        if (!root && (_kind == "Idoc" ? _layouts.ContainsKey(node.Name) : Id(node.Name) is not null)) { yield return node; yield break; }
        foreach (var child in node.Children)
        {
            foreach (var trigger in Triggers(child)) yield return trigger;
            if (!child.Repeating) break;
        }
    }

    public FerruleInstance Parse(byte[] bytes)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        if (bytes.Length > MaximumBytes) throw Fail(FerruleEdiError.Limit, "EDI input exceeds 64 MiB.");
        List<Segment> segments;
        Syntax syntax;
        try { (segments, syntax) = Tokenize(bytes); }
        catch (DecoderFallbackException) { throw Fail(FerruleEdiError.Format, "EDI input is not UTF-8."); }
        if (_strictEnvelope) ValidateEnvelopeSegments(segments);
        int position = 0;
        var source = Read(_root, segments, ref position, syntax, [], true);
        if (!_lenient && position < segments.Count) throw Fail(FerruleEdiError.Schema, "Trailing EDI segment.");
        foreach (var implied in Array(_options, "edi_implied_decimals"))
        {
            string[] path = Array(implied, "path").Select(v => v.GetString()!).ToArray();
            int places = implied.GetProperty("places").GetInt32();
            source = Transform(source, path, 0, value => value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull ? value :
                value.Kind == FerruleValueKind.Double && double.IsFinite(value.DoubleValue / Math.Pow(10, places)) ? FerruleValue.FromDouble(value.DoubleValue / Math.Pow(10, places)) :
                throw Fail(FerruleEdiError.Value, "Implied decimals require a finite Float."));
        }
        return source;
    }

    private bool Matches(Node node, Segment segment)
    {
        if ((_kind == "Idoc" ? node.Name : Id(node.Name)) != segment.Id) return false;
        for (int index = 0; index < node.Children.Length; index++)
        {
            var child = node.Children[index];
            var parts = index < segment.Elements.Count && segment.Elements[index].Count > 0 ? segment.Elements[index][0] : [];
            if (child.Type is not null) { if (child.Fixed is not null && (parts.Count == 0 || parts[0] != child.Fixed)) return false; }
            else for (int at = 0; at < child.Children.Length; at++)
                if (child.Children[at].Fixed is { } fixedValue && (at >= parts.Count || parts[at] != fixedValue)) return false;
        }
        return true;
    }

    private FerruleInstance Read(Node node, List<Segment> segments, ref int position, Syntax syntax, Node[] follow, bool root)
    {
        bool segmentNode = !root && (_kind == "Idoc" ? _layouts.ContainsKey(node.Name) : Id(node.Name) is not null);
        if (segmentNode)
        {
            if (position >= segments.Count) throw Fail(FerruleEdiError.Schema, "Missing required EDI segment.");
            var segment = segments[position++];
            return new FerruleGroup(node.Children.Select((child, at) => {
                var repeats = at < segment.Elements.Count ? segment.Elements[at] : [];
                FerruleInstance value = child.Repeating ? new FerruleRepeated(repeats.Select(parts => ReadElement(child, parts, syntax))) : ReadElement(child, repeats.FirstOrDefault() ?? [], syntax);
                return new FerruleField(child.Name, value);
            }));
        }
        var fields = new List<FerruleField>();
        for (int at = 0; at < node.Children.Length; at++)
        {
            var child = node.Children[at];
            Node[] triggers = Triggers(child).ToArray();
            Node[] childFollow = [.. node.Children.Skip(at + 1).SelectMany(n => Triggers(n)), .. follow];
            Node[] expectations = [.. triggers, .. childFollow];
            bool MatchesAt(Node[] nodes, int cursor) => cursor < segments.Count && nodes.Any(n => Matches(n, segments[cursor]));
            void Skip(ref int cursor) { while (_lenient && cursor < segments.Count && !MatchesAt(expectations, cursor)) cursor++; }
            if (child.Repeating)
            {
                var items = new List<FerruleInstance>();
                while (true)
                {
                    Skip(ref position);
                    if (!MatchesAt(triggers, position)) break;
                    int before = position;
                    items.Add(Read(child, segments, ref position, syntax, [.. triggers, .. childFollow], false));
                    if (position == before) throw Fail(FerruleEdiError.Schema, "EDI loop did not consume a segment.");
                }
                fields.Add(new(child.Name, new FerruleRepeated(items)));
            }
            else
            {
                Skip(ref position);
                if (!MatchesAt(triggers, position)) throw Fail(FerruleEdiError.Schema, "Missing or out-of-order EDI segment.");
                fields.Add(new(child.Name, Read(child, segments, ref position, syntax, childFollow, false)));
            }
        }
        return new FerruleGroup(fields);
    }

    private static FerruleInstance ReadElement(Node node, List<string> parts, Syntax syntax)
    {
        if (node.Type is not null) return new FerruleScalar(ParseScalar(node.Type, DecodeSubcomponent(string.Join(syntax.ReadComponent ?? syntax.Component.ToString(), parts), syntax)));
        return new FerruleGroup(node.Children.Select((child, at) => new FerruleField(child.Name, ReadComponent(child, at < parts.Count ? parts[at] : "", syntax, syntax.Subcomponent))));
    }
    private static FerruleInstance ReadComponent(Node node, string raw, Syntax syntax, char? separator)
    {
        if (node.Type is not null) return new FerruleScalar(ParseScalar(node.Type, DecodeSubcomponent(raw, syntax)));
        string[] parts = separator is { } split ? raw.Split(split) : [raw];
        return new FerruleGroup(node.Children.Select((child, at) => new FerruleField(child.Name, ReadComponent(child, at < parts.Length ? parts[at] : "", syntax, null))));
    }
    private static string DecodeSubcomponent(string raw, Syntax syntax) => syntax.Subcomponent is { } sub && syntax.Release is { } escape ? raw.Replace($"{escape}T{escape}", sub.ToString(), StringComparison.Ordinal) : raw;
    private static FerruleValue ParseScalar(string type, string raw)
    {
        if (raw.Length == 0) return FerruleValue.Null;
        if (type == "string") return FerruleValue.FromString(raw);
        if (raw == raw.Trim())
        {
            if (type == "int" && long.TryParse(raw, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out long integer)) return FerruleValue.FromInt64(integer);
            if (type == "float" && double.TryParse(raw, NumberStyles.Float, CultureInfo.InvariantCulture, out double number) && double.IsFinite(number)) return FerruleValue.FromDouble(number);
            if (type == "bool" && raw is "true" or "false") return FerruleValue.FromBoolean(raw == "true");
        }
        throw Fail(FerruleEdiError.Value, "Cannot parse EDI scalar.");
    }

    private static FerruleInstance Transform(FerruleInstance value, string[] path, int at, Func<FerruleValue, FerruleValue> transform)
    {
        if (value is FerruleRepeated repeated) return new FerruleRepeated(repeated.Items.Select(item => Transform(item, path, at, transform)));
        if (at == path.Length) return value is FerruleScalar scalar ? new FerruleScalar(transform(scalar.Value)) : value;
        return value is FerruleGroup group ? new FerruleGroup(group.Fields.Select(field => new FerruleField(field.Name, field.Name == path[at] ? Transform(field.Value, path, at + 1, transform) : field.Value))) : value;
    }
}
