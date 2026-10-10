using System.Globalization;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    private sealed class Node
    {
        internal required string Name { get; init; }
        internal string? Type { get; init; }
        internal required Node[] Children { get; init; }
        internal bool Repeating { get; init; }
        internal string? Fixed { get; init; }
        internal Range? Length { get; init; }
        internal Range? Count { get; init; }
        internal Constraint? Constraint { get; set; }
        internal byte? ImpliedPlaces { get; set; }
        internal LexicalFormat? Lexical { get; set; }
    }

    private readonly record struct Range(ulong Minimum, ulong? Maximum)
    {
        internal bool Contains(ulong count) => count >= Minimum && (Maximum is null || count <= Maximum);
    }
    private sealed record Constraint(uint Minimum, uint Maximum, HashSet<string> Allowed);
    private sealed record LexicalFormat(string Kind, byte MinimumDigits = 0, byte MaximumDigits = 0, byte MaximumChars = 0);
    private sealed record Autocomplete(bool RequestAcknowledgement, string? TransactionSet);
    private sealed record VersionProfile(string Interchange, string Group)
    {
        internal bool Modern => Interchange != "00401";
    }
    private sealed record Profile(Node Root, Syntax? Separators, bool LenientSegments = false,
        char? ConfiguredRepetition = null, Autocomplete? Autocomplete = null, VersionProfile? Version = null)
    {
        internal VersionProfile SelectedVersion => Version ?? new("00401", "004010");
        internal Syntax OutputSyntax => Separators ?? new('*', ':', '~', SelectedVersion.Modern ? '^' : null);
    }

    private static Profile ParseProfile(string descriptor, bool input = true)
    {
        ArgumentNullException.ThrowIfNull(descriptor);
        RequireUtf8(descriptor, MaximumSchemaBytes, "X12 descriptor");
        try
        {
            using JsonDocument parsed = JsonDocument.Parse(descriptor, new JsonDocumentOptions { MaxDepth = 127 });
            JsonElement wrapper = parsed.RootElement;
            Fields(wrapper, ["version", "schema", "separators", "constraints", "lenient_segments", "implied_decimals", "lexical_formats", "autocomplete"]);
            VersionProfile version = wrapper.GetProperty("version").GetString() switch
            {
                "004010" => new("00401", "004010"),
                "005010" => new("00501", "005010"),
                "006040" => new("00604", "006040"),
                _ => throw Failure(FerruleX12Error.UnsupportedProfile, "Unsupported X12 descriptor version."),
            };
            string schema = wrapper.GetProperty("schema").GetString()
                ?? throw Failure(FerruleX12Error.Schema, "X12 descriptor requires an embedded schema.");
            var decoded = FerruleEmbeddedSchema.Unwrap(schema, MaximumSchemaBytes);
            using JsonDocument schemaDocument = JsonDocument.Parse(decoded.Payload, new JsonDocumentOptions { MaxDepth = 128 });
            int nodeCount = 0;
            Node root = ParseNode(schemaDocument.RootElement, 0, ref nodeCount);
            if (root.Repeating || root.Type is not null)
                throw Failure(FerruleX12Error.Schema, "X12 requires a singular root container.");
            ValidateShape(root, root: true, element: false, component: false, repeated: false);
            var envelopes = new List<Node>();
            CollectEnvelopes(root, envelopes, true);
            string[] ids = ["ISA", "GS", "ST", "SE", "GE", "IEA"];
            int[] widths = [16, 8, 2, 2, 2, 2];
            if (envelopes.Count != 6) throw Failure(FerruleX12Error.Schema, "The schema requires exactly one envelope sequence.");
            for (int index = 0; index < ids.Length; index++)
                if (SegmentId(envelopes[index].Name) != ids[index] || envelopes[index].Children.Length != widths[index]
                    || envelopes[index].Children.Any(child => child.Type != "string" || child.Repeating))
                    throw Failure(FerruleX12Error.Schema, "Envelope schemas require exact positional String elements.");
            if (envelopes[0].Children[11].Fixed != version.Interchange || envelopes[1].Children[7].Fixed != version.Group)
                throw Failure(FerruleX12Error.UnsupportedProfile, version.Modern
                    ? "Envelope schema versions must agree with the selected X12 profile."
                    : "Envelope schema versions must be fixed to 00401 and 004010.");
            Syntax? separators = null;
            char? inactiveRepetition = null;
            JsonElement selected = wrapper.GetProperty("separators");
            if (selected.ValueKind != JsonValueKind.Null)
            {
                Fields(selected, ["element", "component", "segment", "repetition"]);
                separators = new(Character(selected, "element"), Character(selected, "component"), Character(selected, "segment"));
                ValidateSyntax(separators.Value);
                if (selected.TryGetProperty("repetition", out JsonElement repetition) && repetition.ValueKind != JsonValueKind.Null)
                {
                    string? text = repetition.GetString();
                    if (text is null || text.Length != 1)
                        throw Failure(FerruleX12Error.Schema, version.Modern
                            ? "Modern X12 repetition metadata requires one character."
                            : "Inactive X12 repetition metadata requires one character.");
                    char character = text[0];
                    if (!char.IsAscii(character) || character is < '!' or > '~' || char.IsAsciiLetterOrDigit(character)
                        || character == separators.Value.Element || character == separators.Value.Component || character == separators.Value.Segment)
                        throw Failure(FerruleX12Error.Syntax, version.Modern
                            ? "Modern X12 repetition metadata must be distinct ASCII punctuation."
                            : "Inactive X12 repetition metadata must be distinct ASCII punctuation.");
                    inactiveRepetition = character;
                }
                if (version.Modern)
                {
                    separators = separators.Value with { Repetition = inactiveRepetition };
                    if (!input && inactiveRepetition is null)
                        throw Failure(FerruleX12Error.UnsupportedProfile, "Modern X12 output requires a selected repetition separator.");
                }
            }
            JsonElement constraints = wrapper.GetProperty("constraints");
            if (constraints.ValueKind != JsonValueKind.Array || constraints.GetArrayLength() > 10_000)
                throw Failure(FerruleX12Error.Schema, "X12 constraints require a bounded array.");
            foreach (JsonElement constraint in constraints.EnumerateArray())
            {
                Fields(constraint, ["path", "min_chars", "max_chars", "allowed_values"]);
                JsonElement path = constraint.GetProperty("path");
                if (path.ValueKind != JsonValueKind.Array || path.GetArrayLength() is < 1 or > MaximumDepth)
                    throw Failure(FerruleX12Error.Schema, "Invalid X12 constraint path.");
                Node leaf = root;
                foreach (JsonElement part in path.EnumerateArray())
                    leaf = leaf.Children.SingleOrDefault(child => child.Name == part.GetString())
                        ?? throw Failure(FerruleX12Error.Schema, "X12 constraint path does not resolve.");
                uint minimum = constraint.GetProperty("min_chars").GetUInt32();
                uint maximum = constraint.GetProperty("max_chars").GetUInt32();
                if (leaf.Type is null || leaf.Constraint is not null || maximum == 0 || minimum > maximum)
                    throw Failure(FerruleX12Error.Schema, "Invalid or duplicate X12 leaf constraint.");
                var allowed = new HashSet<string>(StringComparer.Ordinal);
                if (constraint.TryGetProperty("allowed_values", out JsonElement values))
                {
                    if (values.ValueKind != JsonValueKind.Array || values.GetArrayLength() > 4_096)
                        throw Failure(FerruleX12Error.Schema, "X12 code lists require a bounded array.");
                    foreach (JsonElement value in values.EnumerateArray())
                        if (!allowed.Add(value.GetString() ?? throw Failure(FerruleX12Error.Schema, "X12 code values require strings.")))
                            throw Failure(FerruleX12Error.Schema, "Duplicate X12 code-list value.");
                }
                leaf.Constraint = new(minimum, maximum, allowed);
            }
            bool lenient = wrapper.TryGetProperty("lenient_segments", out JsonElement leniency) && leniency.GetBoolean();
            ReadProfileMetadata(wrapper, root, input);
            Autocomplete? autocomplete = ReadAutocomplete(wrapper, envelopes[2], input);
            if (version.Modern && envelopes[0].Children[10].Fixed is { } fixedRepetition)
            {
                Syntax? selectedSyntax = separators ?? (!input ? new Syntax('*', ':', '~', '^') : null);
                if (fixedRepetition.Length != 1 || fixedRepetition[0] is < '!' or > '~' || char.IsAsciiLetterOrDigit(fixedRepetition[0])
                    || selectedSyntax is { } physical && (fixedRepetition[0] == physical.Element
                        || fixedRepetition[0] == physical.Component || fixedRepetition[0] == physical.Segment
                        || physical.Repetition is { } repetition && fixedRepetition[0] != repetition))
                    throw Failure(FerruleX12Error.UnsupportedProfile, "Fixed ISA11 does not agree with the selected modern repetition syntax.");
            }
            if (version.Modern && envelopes[0].Children[15].Fixed is { } fixedComponent
                && (fixedComponent.Length != 1 || fixedComponent[0] is < '!' or > '~' || char.IsAsciiLetterOrDigit(fixedComponent[0])
                    || separators is { } physical && fixedComponent != physical.Component.ToString()
                    || separators is null && !input && fixedComponent != ":"))
                throw Failure(FerruleX12Error.UnsupportedProfile, "Fixed ISA16 does not agree with the selected modern component syntax.");
            return new(root, separators, lenient, inactiveRepetition, autocomplete, version);
        }
        catch (FerruleX12Exception) { throw; }
        catch (Exception error) when (error is JsonException or InvalidOperationException or KeyNotFoundException or FormatException or OverflowException)
        {
            throw Failure(FerruleX12Error.Schema, "Invalid embedded X12 descriptor.");
        }
    }

    private static Node ProfileLeaf(Node root, JsonElement path)
    {
        if (path.ValueKind != JsonValueKind.Array || path.GetArrayLength() is < 1 or > MaximumDepth)
            throw Failure(FerruleX12Error.Schema, "Invalid X12 profile option path.");
        Node leaf = root;
        foreach (JsonElement part in path.EnumerateArray())
        {
            string? name = part.GetString();
            if (string.IsNullOrEmpty(name) || name.Any(char.IsControl) || !FerruleUnicode.IsWellFormed(name))
                throw Failure(FerruleX12Error.Schema, "Invalid X12 profile option path component.");
            leaf = leaf.Children.SingleOrDefault(child => child.Name == name)
                ?? throw Failure(FerruleX12Error.Schema, "X12 profile option path does not resolve.");
        }
        if (leaf.Type is null) throw Failure(FerruleX12Error.Schema, "X12 profile options require scalar leaves.");
        return leaf;
    }

    private static JsonElement? ProfileArray(JsonElement wrapper, string name)
    {
        if (!wrapper.TryGetProperty(name, out JsonElement selected)) return null;
        if (selected.ValueKind != JsonValueKind.Array || selected.GetArrayLength() > 10_000)
            throw Failure(FerruleX12Error.Schema, "X12 profile options require bounded arrays.");
        return selected;
    }

    private static void ReadProfileMetadata(JsonElement wrapper, Node root, bool input)
    {
        if (ProfileArray(wrapper, "implied_decimals") is { } implied)
            foreach (JsonElement selected in implied.EnumerateArray())
            {
                Fields(selected, ["path", "places"]);
                Node leaf = ProfileLeaf(root, selected.GetProperty("path"));
                byte places = selected.GetProperty("places").GetByte();
                if (leaf.ImpliedPlaces is not null || places is < 1 or > 18
                    || input && leaf.Type != "float" || !input && leaf.Type is not ("int" or "float"))
                    throw Failure(FerruleX12Error.Schema, "Invalid or duplicate X12 implied-decimal metadata.");
                leaf.ImpliedPlaces = places;
            }
        if (ProfileArray(wrapper, "lexical_formats") is { } lexical)
            foreach (JsonElement selected in lexical.EnumerateArray())
            {
                Fields(selected, ["path", "kind"]);
                Node leaf = ProfileLeaf(root, selected.GetProperty("path"));
                if (leaf.Lexical is not null) throw Failure(FerruleX12Error.Schema, "Duplicate X12 lexical-format metadata.");
                JsonElement kind = selected.GetProperty("kind");
                LexicalFormat format;
                if (kind.ValueKind == JsonValueKind.String)
                {
                    string? value = kind.GetString();
                    if (value is not ("compact_date6" or "compact_date8"))
                        throw Failure(FerruleX12Error.Schema, "Unsupported X12 lexical-format kind.");
                    format = new(value);
                }
                else
                {
                    Fields(kind, ["compact_time", "decimal"]);
                    if (kind.EnumerateObject().Count() != 1)
                        throw Failure(FerruleX12Error.Schema, "X12 lexical-format metadata requires one kind.");
                    if (kind.TryGetProperty("compact_time", out JsonElement time))
                    {
                        Fields(time, ["min_digits", "max_digits"]);
                        byte minimum = time.GetProperty("min_digits").GetByte(), maximum = time.GetProperty("max_digits").GetByte();
                        if (minimum < 4 || minimum > maximum || maximum > 8)
                            throw Failure(FerruleX12Error.Schema, "Invalid X12 compact-time widths.");
                        format = new("compact_time", minimum, maximum);
                    }
                    else
                    {
                        JsonElement number = kind.GetProperty("decimal");
                        Fields(number, ["max_chars"]);
                        byte maximum = number.GetProperty("max_chars").GetByte();
                        if (maximum == 0) throw Failure(FerruleX12Error.Schema, "Invalid X12 decimal maximum length.");
                        format = new("decimal", MaximumChars: maximum);
                    }
                }
                if (!input && format.Kind != "decimal" && leaf.Type != "string")
                    throw Failure(FerruleX12Error.Schema, "X12 output date/time formats require String leaves.");
                leaf.Lexical = format;
            }
    }

    private static Autocomplete? ReadAutocomplete(JsonElement wrapper, Node transaction, bool input)
    {
        if (!wrapper.TryGetProperty("autocomplete", out JsonElement selected) || selected.ValueKind == JsonValueKind.Null) return null;
        Fields(selected, ["request_acknowledgement", "transaction_set"]);
        bool acknowledgement = selected.TryGetProperty("request_acknowledgement", out JsonElement ack) && ack.GetBoolean();
        string? transactionSet = selected.TryGetProperty("transaction_set", out JsonElement configured) ? configured.GetString() : null;
        if (transactionSet is not null && (transactionSet.Length != 3 || !transactionSet.All(char.IsAsciiDigit)))
            throw Failure(FerruleX12Error.Schema, "X12 completion transaction_set requires three ASCII digits.");
        if (!input && transactionSet is not null && transaction.Children[0].Fixed is { Length: > 0 } fixedSet && fixedSet != transactionSet)
            throw Failure(FerruleX12Error.Schema, "X12 completion transaction_set conflicts with fixed ST01.");
        return new(acknowledgement, transactionSet);
    }

    private static Node ParseNode(JsonElement json, int depth, ref int count)
    {
        if (depth > MaximumDepth || ++count > 10_000) throw Limit("X12 schema traversal limit exceeded.");
        Fields(json, ["name", "repeating", "kind", "fixed", "string_length_range", "item_count_range"]);
        string name = json.GetProperty("name").GetString() ?? throw Failure(FerruleX12Error.Schema, "Schema names require strings.");
        if (name.Length == 0 || name.Any(char.IsControl) || !FerruleUnicode.IsWellFormed(name))
            throw Failure(FerruleX12Error.Schema, "Invalid X12 schema field name.");
        JsonElement kind = json.GetProperty("kind");
        string? type = null;
        var children = new List<Node>();
        if (kind.GetProperty("kind").GetString() == "scalar")
        {
            Fields(kind, ["kind", "ty"]);
            type = kind.GetProperty("ty").GetString();
            if (type is not ("string" or "int" or "float"))
                throw Failure(FerruleX12Error.UnsupportedProfile, "X12 scalars support String, Int and finite Float only.");
        }
        else if (kind.GetProperty("kind").GetString() == "group")
        {
            Fields(kind, ["kind", "children"]);
            JsonElement members = kind.GetProperty("children");
            if (members.ValueKind != JsonValueKind.Array || members.GetArrayLength() == 0)
                throw Failure(FerruleX12Error.Schema, "X12 groups require children.");
            var names = new HashSet<string>(StringComparer.Ordinal);
            foreach (JsonElement child in members.EnumerateArray())
            {
                Node parsed = ParseNode(child, depth + 1, ref count);
                if (!names.Add(parsed.Name)) throw Failure(FerruleX12Error.Schema, "Duplicate X12 sibling name.");
                children.Add(parsed);
            }
        }
        else throw Failure(FerruleX12Error.UnsupportedProfile, "Unsupported X12 schema kind.");
        bool repeating = json.TryGetProperty("repeating", out JsonElement repeat) && repeat.GetBoolean();
        string? fixedValue = json.TryGetProperty("fixed", out JsonElement fixedElement) ? fixedElement.GetString() : null;
        Range? length = ReadRange(json, "string_length_range"), cardinality = ReadRange(json, "item_count_range");
        if (fixedValue is not null && type is null || length is not null && type != "string" || cardinality is not null && (!repeating || type is not null))
            throw Failure(FerruleX12Error.Schema, "X12 metadata is attached to an unsupported node kind.");
        if (fixedValue is { Length: > 0 }
            && (type == "int" && !long.TryParse(fixedValue, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out _)
                || type == "float" && (fixedValue != fixedValue.Trim()
                    || !double.TryParse(fixedValue, NumberStyles.Float, CultureInfo.InvariantCulture, out double number)
                    || !double.IsFinite(number))))
            throw Failure(FerruleX12Error.Schema, "X12 numeric fixed metadata requires a valid finite representation.");
        return new() { Name = name, Type = type, Children = children.ToArray(), Repeating = repeating,
            Fixed = fixedValue, Length = length, Count = cardinality };
    }

    private static Range? ReadRange(JsonElement parent, string name)
    {
        if (!parent.TryGetProperty(name, out JsonElement range)) return null;
        Fields(range, ["minimum", "maximum"]);
        ulong minimum = range.TryGetProperty("minimum", out JsonElement min) ? min.GetUInt64() : 0;
        ulong? maximum = range.TryGetProperty("maximum", out JsonElement max) ? max.GetUInt64() : null;
        if (maximum is { } upper && minimum > upper || maximum is null && minimum == 0)
            throw Failure(FerruleX12Error.Schema, "Invalid X12 length/cardinality interval.");
        return new(minimum, maximum);
    }

    private static void Fields(JsonElement json, HashSet<string> allowed)
    {
        if (json.ValueKind != JsonValueKind.Object) throw Failure(FerruleX12Error.Schema, "X12 descriptor members require objects.");
        var seen = new HashSet<string>(StringComparer.Ordinal);
        if (json.EnumerateObject().Any(property => !allowed.Contains(property.Name) || !seen.Add(property.Name)))
            throw Failure(FerruleX12Error.Schema, "Unknown or duplicate X12 descriptor member.");
    }

    private static char Character(JsonElement json, string name)
    {
        string? value = json.GetProperty(name).GetString();
        if (value is null || value.Length != 1) throw Failure(FerruleX12Error.Syntax, "X12 separators require single ASCII characters.");
        return value[0];
    }

    private static string? SegmentId(string name)
    {
        if (name.StartsWith("MF_", StringComparison.Ordinal)) name = name[3..];
        int suffix = name.LastIndexOf('_');
        if (suffix >= 0 && suffix + 1 < name.Length && name[(suffix + 1)..].All(char.IsAsciiDigit)) name = name[..suffix];
        return name.Length is >= 2 and <= 3 && char.IsAsciiLetterUpper(name[0])
            && name.All(character => char.IsAsciiLetterUpper(character) || char.IsAsciiDigit(character)) ? name : null;
    }

    private static void ValidateShape(Node node, bool root, bool element, bool component, bool repeated)
    {
        if (element)
        {
            if (node.Repeating || component && node.Type is null)
                throw Failure(FerruleX12Error.UnsupportedProfile, "004010 elements cannot repeat or contain nested composites.");
            if (node.Children.Length > MaximumElements) throw Limit("X12 composite schema component limit exceeded.");
            if (node.Type is null)
                foreach (Node child in node.Children) ValidateShape(child, false, true, true, false);
            return;
        }
        if (node.Type is not null) throw Failure(FerruleX12Error.Schema, "X12 containers require groups.");
        string? id = root ? null : SegmentId(node.Name);
        if (id is not null)
        {
            if (node.Children.Length > MaximumElements) throw Limit("X12 schema element limit exceeded.");
            if (IsEnvelope(id) && (node.Repeating || repeated))
                throw Failure(FerruleX12Error.UnsupportedProfile, "X12 envelope schemas cannot repeat.");
            foreach (Node child in node.Children) ValidateShape(child, false, true, false, false);
        }
        else foreach (Node child in node.Children) ValidateShape(child, false, false, false, repeated || node.Repeating);
    }

    private static bool IsEnvelope(string id) => id is "ISA" or "GS" or "ST" or "SE" or "GE" or "IEA";
    private static void CollectEnvelopes(Node node, List<Node> result, bool root)
    {
        string? id = root ? null : SegmentId(node.Name);
        if (id is not null) { if (IsEnvelope(id)) result.Add(node); return; }
        foreach (Node child in node.Children) CollectEnvelopes(child, result, false);
    }

    private static void ValidateText(Node node, string text, string[] path)
    {
        if (node.Fixed is not null && text != node.Fixed)
            throw Failure(FerruleX12Error.Value, "X12 fixed value does not match.", path: string.Join('/', path));
        ulong count = (ulong)text.EnumerateRunes().Count();
        if (node.Length is { } length && !length.Contains(count))
            throw Failure(FerruleX12Error.Value, "X12 string length is outside its declared interval.", path: string.Join('/', path));
        if (node.Constraint is { } constraint && (count < constraint.Minimum || count > constraint.Maximum
            || text.Length > 0 && constraint.Allowed.Count > 0 && !constraint.Allowed.Contains(text)))
            throw Failure(FerruleX12Error.Value, "X12 lexical length or code-list constraint failed.", path: string.Join('/', path));
    }

    private static void ValidateCount(Node node, int count, string[] path)
    {
        if (node.Count is { } range && !range.Contains((ulong)count))
            throw Failure(FerruleX12Error.Value, "X12 loop cardinality is outside its declared interval.", path: string.Join('/', path));
    }

}
