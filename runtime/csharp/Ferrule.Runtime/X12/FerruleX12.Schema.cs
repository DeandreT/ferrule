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
    }

    private readonly record struct Range(ulong Minimum, ulong? Maximum)
    {
        internal bool Contains(ulong count) => count >= Minimum && (Maximum is null || count <= Maximum);
    }
    private sealed record Constraint(uint Minimum, uint Maximum, HashSet<string> Allowed);
    private sealed record Profile(Node Root, Syntax? Separators);

    private static Profile ParseProfile(string descriptor)
    {
        ArgumentNullException.ThrowIfNull(descriptor);
        RequireUtf8(descriptor, MaximumSchemaBytes, "X12 descriptor");
        try
        {
            using JsonDocument parsed = JsonDocument.Parse(descriptor, new JsonDocumentOptions { MaxDepth = 127 });
            JsonElement wrapper = parsed.RootElement;
            Fields(wrapper, ["version", "schema", "separators", "constraints"]);
            if (wrapper.GetProperty("version").GetString() != "004010")
                throw Failure(FerruleX12Error.UnsupportedProfile, "Unsupported X12 descriptor version.");
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
            if (envelopes[0].Children[11].Fixed != "00401" || envelopes[1].Children[7].Fixed != "004010")
                throw Failure(FerruleX12Error.UnsupportedProfile, "Envelope schema versions must be fixed to 00401 and 004010.");
            Syntax? separators = null;
            JsonElement selected = wrapper.GetProperty("separators");
            if (selected.ValueKind != JsonValueKind.Null)
            {
                Fields(selected, ["element", "component", "segment"]);
                separators = new(Character(selected, "element"), Character(selected, "component"), Character(selected, "segment"));
                ValidateSyntax(separators.Value);
            }
            JsonElement constraints = wrapper.GetProperty("constraints");
            if (constraints.ValueKind != JsonValueKind.Array) throw Failure(FerruleX12Error.Schema, "X12 constraints require an array.");
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
                    if (values.ValueKind != JsonValueKind.Array) throw Failure(FerruleX12Error.Schema, "X12 code lists require an array.");
                    foreach (JsonElement value in values.EnumerateArray())
                        if (!allowed.Add(value.GetString() ?? throw Failure(FerruleX12Error.Schema, "X12 code values require strings.")))
                            throw Failure(FerruleX12Error.Schema, "Duplicate X12 code-list value.");
                }
                leaf.Constraint = new(minimum, maximum, allowed);
            }
            return new(root, separators);
        }
        catch (FerruleX12Exception) { throw; }
        catch (Exception error) when (error is JsonException or InvalidOperationException or KeyNotFoundException or FormatException or OverflowException)
        {
            throw Failure(FerruleX12Error.Schema, "Invalid embedded X12 descriptor.");
        }
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

    private sealed class Cursor(List<Segment> segments, Syntax syntax, Profile profile)
    {
        internal List<Segment> Segments { get; } = segments;
        internal Syntax Syntax { get; } = syntax;
        internal Profile Profile { get; } = profile;
        internal int Position { get; set; }
        internal int Loops { get; set; }
        internal Budget Budget { get; } = new();
    }

    private static IEnumerable<Node> Triggers(Node node)
    {
        if (SegmentId(node.Name) is not null) { yield return node; yield break; }
        foreach (Node child in node.Children)
        {
            foreach (Node trigger in Triggers(child)) yield return trigger;
            if (!child.Repeating) break;
        }
    }

    private static bool Matches(Node node, Segment segment, Syntax syntax)
    {
        if (SegmentId(node.Name) != segment.Id) return false;
        for (int index = 0; index < node.Children.Length; index++)
        {
            Node element = node.Children[index];
            string raw = index < segment.Elements.Length ? segment.Elements[index] : "";
            if (element.Type is not null)
            {
                if (element.Fixed is not null && raw != element.Fixed) return false;
            }
            else
            {
                string[] components = raw.Split(syntax.Component);
                for (int at = 0; at < element.Children.Length; at++)
                    if (element.Children[at].Fixed is { } fixedValue
                        && (at < components.Length ? components[at] : "") != fixedValue) return false;
            }
        }
        return true;
    }

    private static FerruleInstance ReadContainer(Node node, Cursor cursor, string[] path, int depth, bool root = false)
    {
        cursor.Budget.Visit(depth);
        if (!root && SegmentId(node.Name) is not null)
        {
            if (cursor.Position >= cursor.Segments.Count) throw Failure(FerruleX12Error.Schema, "Missing required X12 segment.", cursor.Position);
            Segment segment = cursor.Segments[cursor.Position];
            if (!Matches(node, segment, cursor.Syntax)) throw Failure(FerruleX12Error.Schema, "X12 segment or fixed qualifier does not match the schema.", cursor.Position);
            if (segment.Elements.Length > node.Children.Length) throw Failure(FerruleX12Error.Schema, "X12 segment contains undeclared elements.", cursor.Position);
            var fields = new List<FerruleField>();
            for (int index = 0; index < node.Children.Length; index++)
                fields.Add(new(node.Children[index].Name, ReadElement(node.Children[index],
                    index < segment.Elements.Length ? segment.Elements[index] : "", cursor, [.. path, node.Children[index].Name], depth + 1,
                    segment.Id == "ISA" && index == 15)));
            cursor.Position++;
            return new FerruleGroup(fields);
        }
        var children = new List<FerruleField>();
        foreach (Node child in node.Children)
        {
            Node[] triggers = Triggers(child).ToArray();
            string[] childPath = [.. path, child.Name];
            if (child.Repeating)
            {
                var items = new List<FerruleInstance>();
                while (cursor.Position < cursor.Segments.Count && triggers.Any(trigger => Matches(trigger, cursor.Segments[cursor.Position], cursor.Syntax)))
                {
                    if (++cursor.Loops > MaximumLoopInstances) throw Limit("X12 loop instance limit exceeded.");
                    int before = cursor.Position;
                    items.Add(ReadContainer(child, cursor, childPath, depth + 1));
                    if (before == cursor.Position) throw Failure(FerruleX12Error.Schema, "An X12 loop failed to consume a segment.", cursor.Position);
                }
                ValidateCount(child, items.Count, childPath);
                children.Add(new(child.Name, new FerruleRepeated(items)));
            }
            else children.Add(new(child.Name, ReadContainer(child, cursor, childPath, depth + 1)));
        }
        return new FerruleGroup(children);
    }

    private static FerruleInstance ReadElement(Node node, string raw, Cursor cursor, string[] path, int depth, bool isaComponent = false)
    {
        cursor.Budget.Visit(depth);
        if (node.Type is not null)
        {
            if (!isaComponent && raw.Contains(cursor.Syntax.Component))
                throw Failure(FerruleX12Error.Schema, "X12 scalar contains undeclared composite syntax.", cursor.Position, string.Join('/', path));
            return new FerruleScalar(ReadScalar(node, raw, path));
        }
        if (raw.Count(character => character == cursor.Syntax.Component) >= node.Children.Length)
            throw Failure(FerruleX12Error.Schema, "X12 composite contains undeclared components.", cursor.Position, string.Join('/', path));
        string[] components = raw.Split(cursor.Syntax.Component);
        return new FerruleGroup(node.Children.Select((child, index) => new FerruleField(child.Name,
            ReadElement(child, index < components.Length ? components[index] : "", cursor, [.. path, child.Name], depth + 1))));
    }

    private static FerruleValue ReadScalar(Node node, string raw, string[] path)
    {
        ValidateText(node, raw, path);
        if (raw.Length == 0) return FerruleValue.Null;
        if (node.Type == "string") return FerruleValue.FromString(raw);
        if (node.Type == "int" && long.TryParse(raw, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out long integer))
            return FerruleValue.FromInt64(integer);
        if (node.Type == "float" && raw == raw.Trim() && double.TryParse(raw, NumberStyles.Float, CultureInfo.InvariantCulture, out double number) && double.IsFinite(number))
            return FerruleValue.FromDouble(number);
        throw Failure(FerruleX12Error.Value, "X12 scalar has an invalid numeric representation.", path: string.Join('/', path));
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

    private static void WriteContainer(Node node, FerruleInstance instance, Profile profile, string[] path,
        List<Segment> segments, Budget budget, int depth, bool root = false)
    {
        budget.Visit(depth);
        if (instance is not FerruleGroup group) throw Failure(FerruleX12Error.Value, "X12 requires a group instance.", path: string.Join('/', path));
        if (group.Fields.Any(field => !node.Children.Any(child => child.Name == field.Name)))
            throw Failure(FerruleX12Error.Value, "X12 instance contains an undeclared field.", path: string.Join('/', path));
        string? id = root ? null : SegmentId(node.Name);
        if (id is not null)
        {
            if (segments.Count == MaximumSegments) throw Limit("X12 output segment limit exceeded.");
            Syntax syntax = profile.Separators ?? new('*', ':', '~');
            int control = id switch { "ISA" => 12, "GS" => 5, "ST" or "SE" or "GE" or "IEA" => 1, _ => -1 };
            if (control >= 0 && (!group.TryGetField(node.Children[control].Name, out FerruleInstance? supplied)
                || supplied is not FerruleScalar { Value.Kind: FerruleValueKind.String } scalar || scalar.Value.StringValue.Length == 0))
                throw Failure(FerruleX12Error.Envelope, "X12 control numbers must be supplied explicitly.", path: string.Join('/', path));
            var elements = new string[node.Children.Length];
            long textBytes = 0;
            long wireBytes = id.Length + 1 + (syntax.Segment == '\n' ? 0 : 1);
            for (int index = 0; index < elements.Length; index++)
            {
                Node child = node.Children[index];
                string text = WriteElement(child,
                    group.TryGetField(child.Name, out FerruleInstance? value) ? value : null,
                    syntax, [.. path, child.Name], budget, depth + 1, id == "ISA" && index == 15,
                    Math.Max(0, budget.RemainingOutput - id.Length - 1 - (syntax.Segment == '\n' ? 0 : 1)
                        - textBytes - index - 1));
                elements[index] = text;
                textBytes += StrictUtf8.GetByteCount(text);
                if (id == "ISA" && index is 1 or 3 or 5 or 7)
                    textBytes += Math.Max(0, IsaWidths[index] - text.Length);
                // Trailing empty body elements are omitted, but internal empty
                // positions and every ISA delimiter still occupy wire bytes.
                if (id == "ISA" || text.Length > 0)
                    wireBytes = id.Length + 1 + (syntax.Segment == '\n' ? 0 : 1) + textBytes + index + 1;
                budget.CheckOutput(wireBytes);
            }
            budget.AddOutput(wireBytes);
            segments.Add(new(id, elements));
            return;
        }
        foreach (Node child in node.Children)
        {
            group.TryGetField(child.Name, out FerruleInstance? value);
            string[] childPath = [.. path, child.Name];
            if (child.Repeating)
            {
                IReadOnlyList<FerruleInstance> items = value switch
                {
                    null => [],
                    FerruleRepeated repeated => repeated.Items,
                    FerruleMappedSequence sequence => sequence.Items,
                    _ => throw Failure(FerruleX12Error.Value, "X12 repeating fields require an ordered collection.", path: string.Join('/', childPath)),
                };
                if (items.Count > MaximumLoopInstances) throw Limit("X12 output loop instance limit exceeded.");
                ValidateCount(child, items.Count, childPath);
                foreach (FerruleInstance item in items)
                {
                    budget.Loop();
                    WriteContainer(child, item, profile, childPath, segments, budget, depth + 1);
                }
            }
            else WriteContainer(child, value ?? throw Failure(FerruleX12Error.Value, "X12 output is missing a required group.", path: string.Join('/', childPath)),
                profile, childPath, segments, budget, depth + 1);
        }
    }

    private static string WriteElement(Node node, FerruleInstance? instance, Syntax syntax, string[] path,
        Budget budget, int depth, bool isaComponent = false, long maximumBytes = MaximumDocumentBytes)
    {
        budget.Visit(depth);
        if (node.Type is null)
        {
            if (instance is not null && instance is not FerruleGroup) throw Failure(FerruleX12Error.Value, "X12 composite requires a group.", path: string.Join('/', path));
            var group = instance as FerruleGroup;
            if (group is not null && group.Fields.Any(field => !node.Children.Any(child => child.Name == field.Name)))
                throw Failure(FerruleX12Error.Value, "X12 composite contains an undeclared field.", path: string.Join('/', path));
            var components = node.Children.Select(child => WriteElement(child,
                group is not null && group.TryGetField(child.Name, out FerruleInstance? value) ? value : null,
                syntax, [.. path, child.Name], budget, depth + 1)).ToList();
            while (components.Count > 0 && components[^1].Length == 0) components.RemoveAt(components.Count - 1);
            long bytes = Math.Max(0, components.Count - 1);
            foreach (string component in components)
            {
                bytes += StrictUtf8.GetByteCount(component);
                if (bytes > maximumBytes) throw Limit("X12 composite byte limit exceeded.");
            }
            return string.Join(syntax.Component, components);
        }
        if (instance is not null && instance is not FerruleScalar) throw Failure(FerruleX12Error.Value, "X12 leaf requires a scalar.", path: string.Join('/', path));
        FerruleValue scalar = instance is FerruleScalar leaf ? leaf.Value : FerruleValue.Null;
        string text;
        if (scalar.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull
            || scalar.Kind == FerruleValueKind.String && scalar.StringValue.Length == 0 && node.Fixed is not null) text = node.Fixed ?? "";
        else if (node.Type == "string" && scalar.Kind == FerruleValueKind.String) text = scalar.StringValue;
        else if (node.Type == "int" && scalar.Kind == FerruleValueKind.Int64) text = scalar.Int64Value.ToString(CultureInfo.InvariantCulture);
        else if (node.Type == "float" && scalar.Kind is FerruleValueKind.Double or FerruleValueKind.Int64)
        {
            double number = scalar.Kind == FerruleValueKind.Double ? scalar.DoubleValue : scalar.Int64Value;
            if (!double.IsFinite(number)) throw Failure(FerruleX12Error.Value, "X12 numbers must be finite.", path: string.Join('/', path));
            text = FerruleValueMaps.RustFloatText(number);
        }
        else throw Failure(FerruleX12Error.Value, "X12 scalar type does not match the schema.", path: string.Join('/', path));
        if (node.Fixed is { } fixedText && text != fixedText && node.Type is not null and not "string")
        {
            FerruleValue fixedValue = ReadScalar(node, fixedText, path);
            bool same = node.Type == "int"
                ? long.TryParse(text, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out long integer)
                    && fixedValue.Kind == FerruleValueKind.Int64 && fixedValue.Int64Value == integer
                : double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out double number)
                    && fixedValue.Kind == FerruleValueKind.Double && fixedValue.DoubleValue == number;
            if (same) text = fixedText;
        }
        RequireUtf8(text, MaximumDocumentBytes, "X12 field");
        ValidateText(node, text, path);
        if (!(isaComponent && text == syntax.Component.ToString())
            && text.Any(character => char.IsControl(character) || character == syntax.Element || character == syntax.Component || character == syntax.Segment))
            throw Failure(FerruleX12Error.Value, "X12 scalar contains an unrepresentable delimiter.", path: string.Join('/', path));
        return text;
    }
}
