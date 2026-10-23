using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    /// <summary>Explicit MapForce-compatible field formatting, required layout
    /// materialization and Latin-1 output. No file is opened or accepted here.</summary>
    public static byte[] SerializeMapForceDocumentBytes(string descriptor, FerruleInstance document, FerruleExecutionContext execution)
    {
        ArgumentNullException.ThrowIfNull(document); ArgumentNullException.ThrowIfNull(execution);
        RequireUtf8(descriptor, 2 * MaximumSchemaBytes, "MapForce writer descriptor");
        using var parsed = JsonDocument.Parse(descriptor);
        var wrapper = parsed.RootElement; Fields(wrapper, ["boundary", "minimums"]);
        var profile = ParseProfile(wrapper.GetProperty("boundary").GetString()!, input: false);
        if (profile.MapForce is null || profile.OutputSyntax.Segment == '\n')
            throw Failure(FerruleX12Error.Schema, "Requires MapForce X12 ownership and visible terminator.");
        var minimums = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (var value in wrapper.GetProperty("minimums").EnumerateArray())
        {
            Fields(value, ["path", "minimum"]);
            var path = value.GetProperty("path").EnumerateArray().Select(p => p.GetString()!).ToArray();
            if (path.Length > 64 || path.Any(p => p.Contains('\0')) || minimums.Count >= 10000)
                throw Failure(FerruleX12Error.Schema, "Invalid writer minimum path.");
            var node = profile.Root;
            foreach (var part in path) node = node.Children.SingleOrDefault(n => n.Name == part) ?? throw Failure(FerruleX12Error.Schema, "Unknown writer minimum path.");
            int minimum = value.GetProperty("minimum").GetInt32();
            if (minimum is < 0 or > 1024 || node.Type is not null || !minimums.TryAdd(string.Join('\0', path), minimum))
                throw Failure(FerruleX12Error.Schema, "Invalid writer minimum.");
        }
        var segments = new List<Segment>(); var budget = new Budget();
        MapForceContainer(profile.Root, document, profile, minimums, [], segments, budget, 0, false, true);
        var automaticTimes = new Queue<bool>(segments.Where(s => s.Id == "GS").Select(s => s.Elements[4].Length == 0));
        if (profile.Autocomplete is not null) segments = CompleteMapForceSegments(profile, segments, execution);
        foreach (var segment in segments.Where(s => s.Id == "GS"))
            if (automaticTimes.Dequeue() && profile.Autocomplete is not null) segment.Elements[4] = segment.Elements[4][..4];
        EncodeInterchangeControls(segments);
        var output = new StringBuilder(); long bytes = 0;
        foreach (var segment in segments)
        {
            int length = segment.Elements.Length;
            if (segment.Id != "ISA") while (length > 0 && segment.Elements[length - 1].Length == 0) length--;
            Add(segment.Id);
            for (int index = 0; index < length; index++) { Add(profile.OutputSyntax.Element.ToString()); Add(segment.Elements[index]); }
            Add(profile.OutputSyntax.Segment + "\r\n");
        }
        return Encoding.GetEncoding("ISO-8859-1").GetBytes(output.ToString());
        void Add(string text) { bytes += StrictUtf8.GetByteCount(text); if (bytes > MaximumDocumentBytes) throw Limit("MapForce output byte limit exceeded."); output.Append(text); }
    }

    private static void MapForceContainer(Node node, FerruleInstance? instance, Profile profile,
        Dictionary<string, int> minimums, string[] path, List<Segment> segments, Budget budget, int depth, bool item, bool root = false)
    {
        budget.Visit(depth); int minimum = minimums.GetValueOrDefault(string.Join('\0', path));
        if (node.Repeating && !item)
        {
            IReadOnlyList<FerruleInstance> items = instance switch {
                null => [], FerruleRepeated r => r.Items, FerruleMappedSequence s => s.Items,
                _ => throw Failure(FerruleX12Error.Value, "Expected repeated writer group."),
            };
            if (path.Length == 1 && items.Count == 0) throw Failure(FerruleX12Error.Envelope, "No output interchange.");
            int count = Math.Max(items.Count, minimum);
            for (int index = 0; index < count; index++) { budget.Loop(); MapForceContainer(node, index < items.Count ? items[index] : new FerruleGroup([]), profile, minimums, path, segments, budget, depth + 1, true); }
            return;
        }
        if (instance is null && minimum == 0 && !root) return;
        if (instance is not null and not FerruleGroup) throw Failure(FerruleX12Error.Value, "Expected writer group.");
        var group = instance as FerruleGroup ?? new FerruleGroup([]);
        if (group.Fields.Any(f => !node.Children.Any(c => c.Name == f.Name))) throw Failure(FerruleX12Error.Value, "Unknown writer field.");
        var id = root ? null : SegmentId(node.Name);
        if (id is not null)
        {
            if (segments.Count == MaximumSegments) throw Limit("Too many output segments.");
            var values = new string[node.Children.Length];
            for (int i = 0; i < values.Length; i++)
            {
                var child = node.Children[i]; group.TryGetField(child.Name, out var value);
                char? allowed = id == "ISA" ? i == 10 ? profile.OutputSyntax.Repetition : i == 15 ? profile.OutputSyntax.Component : null : null;
                values[i] = MapForceElement(child, value, profile.OutputSyntax, budget, depth + 1, allowed);
                if (id == "ISA" && values[i].Length == 0)
                    values[i] = i switch { 10 => profile.OutputSyntax.Repetition?.ToString() ?? "", 11 => profile.SelectedVersion.Interchange, 15 => profile.OutputSyntax.Component.ToString(), _ => "" };
            }
            segments.Add(new(id, values)); return;
        }
        foreach (var child in node.Children)
        {
            group.TryGetField(child.Name, out var value);
            MapForceContainer(child, value, profile, minimums, [..path, child.Name], segments, budget, depth + 1, false);
        }
    }

    private static string MapForceElement(Node node, FerruleInstance? instance, Syntax syntax, Budget budget, int depth, char? allowed = null, bool item = false)
    {
        budget.Visit(depth);
        if (node.Repeating && !item)
        {
            if (instance is null) return "";
            if (instance is not FerruleRepeated repeat || syntax.Repetition is not { } separator) throw Failure(FerruleX12Error.Value, "Expected repeated element with repetition syntax.");
            var values = new List<string>();
            foreach (var candidate in repeat.Items) { budget.Loop(); values.Add(MapForceElement(node, candidate, syntax, budget, depth + 1, allowed, true)); }
            return string.Join(separator, values);
        }
        if (node.Type is null)
        {
            if (instance is not null and not FerruleGroup) throw Failure(FerruleX12Error.Value, "Expected composite.");
            var group = instance as FerruleGroup;
            var values = node.Children.Select(child => MapForceElement(child, group is not null && group.TryGetField(child.Name, out var found) ? found : null, syntax, budget, depth + 1)).ToList();
            while (values.Count > 0 && values[^1].Length == 0) values.RemoveAt(values.Count - 1);
            return string.Join(syntax.Component, values);
        }
        if (instance is not null and not FerruleScalar) throw Failure(FerruleX12Error.Value, "Expected scalar.");
        var value = instance is FerruleScalar scalar ? scalar.Value : node.Fixed is not null ? FerruleValue.FromString(node.Fixed) : FerruleValue.Null;
        if (value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull || value.Kind == FerruleValueKind.String && value.StringValue.Length == 0) return "";
        var kind = node.Lexical?.Kind switch {
            "compact_date6" or "compact_date8" => FerruleMapForceEdiScalarKind.Date,
            "compact_time" => FerruleMapForceEdiScalarKind.Time,
            "decimal" => FerruleMapForceEdiScalarKind.Decimal,
            _ => node.Type is "int" or "float" ? FerruleMapForceEdiScalarKind.Decimal : FerruleMapForceEdiScalarKind.Text,
        };
        var field = new FerruleMapForceEdiScalar(kind, (int)(node.Constraint?.Minimum ?? 0), (int)(node.Constraint?.Maximum ?? 65536));
        string text = FerruleMapForceEdiScalars.Format(field, value);
        budget.AddOutput(StrictUtf8.GetByteCount(text));
        if (text.Length == 1 && text[0] == allowed) return text;
        return new string(text.Select(c => c == syntax.Element || c == syntax.Component || c == syntax.Segment || c == syntax.Repetition ? ' ' : c).ToArray());
    }
}
