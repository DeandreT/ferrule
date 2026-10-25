using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    private sealed record NativeParticle(Node Node, string? Segment, uint Minimum, uint? Maximum, bool RespectMaximum, uint MergedEntries, NativeParticle[] Children, bool Selection, string[] KnownSegments)
    {
        internal static NativeParticle Read(JsonElement value, Node node, int depth, ref int count)
        {
            if (depth > 64 || ++count > 10000 || value.ValueKind != JsonValueKind.Object) throw Failure(FerruleX12Error.Schema, "Invalid native X12 structure.");
            string[] names = value.EnumerateObject().Select(p => p.Name).ToArray();
            if (names.Length is < 7 or > 9 || names.Distinct(StringComparer.Ordinal).Count() != names.Length || names.Any(n => n is not ("name" or "segment" or "minimum" or "maximum" or "respect_maximum" or "merged_entries" or "children" or "selection" or "known_segments"))) throw Failure(FerruleX12Error.Schema, "Invalid native X12 structure fields.");
            foreach (string required in new[] { "name", "segment", "minimum", "maximum", "respect_maximum", "merged_entries", "children" }) if (!value.TryGetProperty(required, out _)) throw Failure(FerruleX12Error.Schema, "Missing native X12 structure field.");
            var name = value.GetProperty("name"); var segment = value.GetProperty("segment"); var minimum = value.GetProperty("minimum"); var maximum = value.GetProperty("maximum"); var respect = value.GetProperty("respect_maximum"); var merged = value.GetProperty("merged_entries"); var children = value.GetProperty("children");
            if (name.ValueKind != JsonValueKind.String || name.GetString() != node.Name || node.Type is not null || minimum.ValueKind != JsonValueKind.Number || !minimum.TryGetUInt32(out uint min) || min > 1024 || merged.ValueKind != JsonValueKind.Number || !merged.TryGetUInt32(out uint entries) || entries is 0 or > 1024 || respect.ValueKind is not (JsonValueKind.True or JsonValueKind.False) || children.ValueKind != JsonValueKind.Array) throw Failure(FerruleX12Error.Schema, "Invalid native X12 structure binding.");
            uint? max = null;
            if (maximum.ValueKind != JsonValueKind.Null) { if (maximum.ValueKind != JsonValueKind.Number || !maximum.TryGetUInt32(out uint finite) || finite < min) throw Failure(FerruleX12Error.Schema, "Invalid native X12 maximum."); max = finite; }
            string? id = null;
            if (segment.ValueKind != JsonValueKind.Null) { if (segment.ValueKind != JsonValueKind.String) throw Failure(FerruleX12Error.Schema, "Invalid native segment."); id = segment.GetString()!; if (id.Length is < 2 or > 3 || id.Any(c => !(char.IsAsciiLetterUpper(c) || char.IsAsciiDigit(c))) || children.GetArrayLength() != 0) throw Failure(FerruleX12Error.Schema, "Invalid native segment."); }
            var childPlans = new List<NativeParticle>();
            bool selection = false;
            if (value.TryGetProperty("selection", out var selected)) { if (selected.ValueKind is not (JsonValueKind.True or JsonValueKind.False)) throw Failure(FerruleX12Error.Schema, "Invalid native selection."); selection = selected.GetBoolean(); if (selection && id is not null) throw Failure(FerruleX12Error.Schema, "Invalid native selection."); }
            string[] known = [];
            if (value.TryGetProperty("known_segments", out var catalog))
            {
                if (catalog.ValueKind != JsonValueKind.Array || catalog.GetArrayLength() > 10000 || (depth != 0 && catalog.GetArrayLength() != 0) || catalog.EnumerateArray().Any(item => item.ValueKind != JsonValueKind.String)) throw Failure(FerruleX12Error.Schema, "Invalid native segment catalog.");
                known = catalog.EnumerateArray().Select(item => item.GetString()!).ToArray();
                if (known.Distinct(StringComparer.Ordinal).Count() != known.Length || known.Any(item => item.Length is < 2 or > 3 || item.Any(c => !(char.IsAsciiLetterUpper(c) || char.IsAsciiDigit(c))))) throw Failure(FerruleX12Error.Schema, "Invalid native segment catalog.");
            }
            if (id is null)
            {
                if (children.GetArrayLength() != node.Children.Length) throw Failure(FerruleX12Error.Schema, "Native X12 structure does not cover its schema.");
                int index = 0;
                foreach (var child in children.EnumerateArray()) childPlans.Add(Read(child, node.Children[index++], depth + 1, ref count));
            }
            var result = new NativeParticle(node, id, min, max, respect.GetBoolean(), entries, childPlans.ToArray(), selection, known);
            if (depth == 0 && result.HasSelection && known.Length == 0) throw Failure(FerruleX12Error.Schema, "Native selection requires a segment catalog.");
            return result;
        }
        private bool HasSelection => Selection || Children.Any(child => child.HasSelection);
    }

    private static FerruleInstance ParseNativeStructureSource(Profile profile, JsonElement structure, byte[] bytes, string encoding)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        if (bytes.Length > MaximumDocumentBytes) throw Limit("X12 source byte limit exceeded.");
        bool bom = bytes.AsSpan().StartsWith(new byte[] { 0xef, 0xbb, 0xbf }) || bytes.AsSpan().StartsWith(new byte[] { 0xff, 0xfe }) || bytes.AsSpan().StartsWith(new byte[] { 0xfe, 0xff }) || bytes.AsSpan().StartsWith(new byte[] { 0, 0, 0xfe, 0xff });
        string text = encoding == "latin1" && !bom ? Encoding.Latin1.GetString(bytes) : FerruleTextDecoding.BomAwareNonFlushing(bytes);
        RequireUtf8(text, MaximumDocumentBytes, "X12 source");
        int count = 0;
        var plan = NativeParticle.Read(structure, LexicalNode(profile.Root), 0, ref count);
        var reader = new NativeSourceReader(text, plan.KnownSegments);
        reader.Skip();
        if (reader.Remaining.StartsWith("ISA", StringComparison.Ordinal)) reader.Discover();
        return reader.Read(plan, 0);
    }

    private sealed class NativeSourceReader(string text, string[] knownSegments)
    {
        private int position;
        private int nodes;
        private int segments;
        private Syntax syntax = new('*', ':', '~');
        internal string Remaining => text[position..];
        private bool Separator(char ch) => ch == syntax.Element || ch == syntax.Component || ch == syntax.Segment || ch == syntax.Repetition || ch == '\0';
        private bool Ignorable(char ch) => ch is '\t' or '\r' or '\n' && !Separator(ch);
        internal void Skip() { while (position < text.Length && char.IsWhiteSpace(text[position]) && !Separator(text[position])) position++; }
        private string Tag()
        {
            string remaining = Remaining;
            if (remaining.StartsWith("ISA", StringComparison.Ordinal)) return "ISA";
            var value = new StringBuilder(); bool escaped = false;
            foreach (char ch in remaining)
            {
                if (Ignorable(ch)) continue;
                if (escaped) { value.Append(ch); escaped = false; }
                else if (ch == '\0') escaped = true;
                else if (Separator(ch)) break;
                else value.Append(ch);
                if (value.Length > 32) break;
            }
            return value.ToString();
        }
        internal void Discover()
        {
            string remaining = Remaining;
            if (remaining.Length < 4) throw Failure(FerruleX12Error.Syntax, "Truncated native ISA.");
            char element = remaining[3];
            string[] fields = remaining.Split(element, 17);
            if (fields.Length != 17 || fields[16].Length < 2) throw Failure(FerruleX12Error.Syntax, "Truncated native ISA.");
            char? repetition = fields[11].Length > 0 && !char.IsLetterOrDigit(fields[11][0]) ? fields[11][0] : null;
            syntax = new(element, fields[16][0], fields[16][1], repetition);
        }
        // Recognizing a selection header and resolving its qualifier are separate.
        private static bool StartsWithTag(NativeParticle plan, string tag)
        {
            if (plan.Segment is { } id) return id == tag;
            foreach (var child in plan.Children)
            {
                if (StartsWithTag(child, tag)) return true;
                if (child.Minimum > 0) break;
            }
            return false;
        }
        private bool Matches(NativeParticle plan)
        {
            if (plan.Segment is { } id)
            {
                if (Tag() != id) return false;
                if (id == "ISA") return true;
                if (plan.Node.Children.All(child => child.Fixed is null)) return true;
                string[][][] fields;
                try { fields = Scan(id, false).Elements; }
                catch (FerruleX12Exception) { return false; }
                for (int i = 0; i < plan.Node.Children.Length; i++) if (plan.Node.Children[i].Fixed is { } fixedValue && (i < fields.Length ? fields[i].FirstOrDefault()?.FirstOrDefault() ?? "" : "") != fixedValue) return false;
                return true;
            }
            // Group admission sees the selection header; selection reading resolves its qualifier.
            foreach (var child in plan.Children)
            {
                if ((child.Selection && StartsWithTag(child, Tag())) || Matches(child)) return true;
                if (child.Minimum > 0) break;
            }
            return false;
        }
        private (string[][][] Elements, int Consumed) Scan(string id, bool required)
        {
            var fields = new List<List<List<StringBuilder>>> { new() { new() { new() } } };
            bool escaped = false; bool terminated = false; int consumed = 0; int tokens = 1;
            foreach (char ch in Remaining)
            {
                consumed++;
                if (Ignorable(ch)) continue;
                if (escaped) { fields[^1][^1][^1].Append(ch); escaped = false; }
                else if (ch == '\0') { if (id == "ISA") throw Failure(FerruleX12Error.Syntax, "NUL in native ISA."); escaped = true; }
                else if (ch == syntax.Segment) { terminated = true; break; }
                else if (ch == syntax.Element) { fields.Add(new() { new() { new() } }); tokens += 2; }
                else if (id != "ISA" && ch == syntax.Repetition) { fields[^1].Add(new() { new() }); tokens += 2; if (fields[^1].Count > MaximumLoopInstances) throw Limit("Native X12 repetition limit exceeded."); }
                else if (id != "ISA" && ch == syntax.Component) { fields[^1][^1].Add(new()); tokens++; if (fields[^1][^1].Count > MaximumElements) throw Limit("Native X12 component limit exceeded."); }
                else fields[^1][^1][^1].Append(ch);
                if (fields.Count - 1 > MaximumElements || nodes > MaximumNodes - tokens) throw Limit("Native X12 token limit exceeded.");
            }
            if (required && !terminated) throw Failure(FerruleX12Error.Syntax, "Unterminated native X12 segment.");
            return (fields.Skip(1).Select(repeats => repeats.Select(parts => parts.Select(value => value.ToString()).ToArray()).ToArray()).ToArray(), consumed);
        }
        private sealed class FieldCursor
        {
            private readonly List<(char Kind, string? Value)> tokens = [];
            private int at;
            internal FieldCursor(string[][][] elements)
            {
                for (int e = 0; e < elements.Length; e++)
                {
                    if (e > 0) tokens.Add(('e', null));
                    for (int r = 0; r < elements[e].Length; r++)
                    {
                        if (r > 0) tokens.Add(('r', null));
                        for (int c = 0; c < elements[e][r].Length; c++)
                        {
                            if (c > 0) tokens.Add(('c', null));
                            tokens.Add(('t', elements[e][r][c]));
                        }
                    }
                }
            }
            private bool Has(char kind) => at < tokens.Count && tokens[at].Kind == kind;
            internal void ElementSeparator() { if (Has('e')) at++; }
            private FerruleInstance Item(Node node)
            {
                if (node.Type is not null) return new FerruleScalar(Has('t') ? FerruleValue.FromString(tokens[at++].Value!) : FerruleValue.Null);
                var fields = new List<FerruleField>();
                for (int i = 0; i < node.Children.Length; i++)
                {
                    if (i > 0 && Has('c')) at++;
                    fields.Add(new(node.Children[i].Name, Item(node.Children[i])));
                }
                return new FerruleGroup(fields);
            }
            internal FerruleInstance Field(Node node)
            {
                if (!node.Repeating) return Item(node);
                var items = new List<FerruleInstance>();
                if (Has('t') || Has('c'))
                {
                    items.Add(Item(node));
                    while (Has('r')) { at++; items.Add(Item(node)); }
                }
                return new FerruleRepeated(items);
            }
        }
        internal FerruleInstance Read(NativeParticle plan, int depth)
        {
            if (depth > 64 || ++nodes > MaximumNodes) throw Limit("Native X12 structure limit exceeded.");
            if (plan.Segment is { } id)
            {
                Skip(); if (id == "ISA") Discover();
                var scanned = Scan(id, true); var elements = scanned.Elements;
                int tokens = elements.Sum(repeats => 1 + repeats.Sum(parts => 1 + parts.Length));
                if (nodes > MaximumNodes - tokens) throw Limit("Native X12 token limit exceeded.");
                nodes += tokens;
                var fields = new List<FerruleField>();
                var cursor = new FieldCursor(elements);
                for (int i = 0; i < plan.Node.Children.Length; i++)
                {
                    if (i > 0) cursor.ElementSeparator();
                    Node node = plan.Node.Children[i];
                    fields.Add(new(node.Name, cursor.Field(node)));
                }
                position += scanned.Consumed; if (++segments > MaximumSegments) throw Limit("Native X12 segment limit exceeded.");
                return new FerruleGroup(fields);
            }
            var children = new List<FerruleField>();
            foreach (var child in plan.Children)
            {
                uint? maximum = child.MergedEntries > 1 ? child.MergedEntries : child.RespectMaximum || child.Maximum is <= 1 ? child.Maximum : null;
                var items = new List<FerruleInstance>();
                while (maximum is null || items.Count < maximum.Value)
                {
                    Skip(); if (position == text.Length) break;
                    if (!Matches(child))
                    {
                        string tag = Tag();
                        if (child.Selection && StartsWithTag(child, tag)) throw Failure(FerruleX12Error.Syntax, "Unrecognized native X12 selection qualifier.");
                        if (child.Selection && tag.Length != 0 && !knownSegments.Contains(tag, StringComparer.Ordinal)) throw Failure(FerruleX12Error.Syntax, "Unrecognized segment at native X12 selection.");
                        break;
                    }
                    int before = position; items.Add(Read(child, depth + 1));
                    if (before == position) throw Failure(FerruleX12Error.Schema, "Native X12 particle made no progress.");
                }
                if (items.Count < child.Minimum) throw Failure(FerruleX12Error.Syntax, "Missing required native X12 particle.");
                children.Add(new(child.Node.Name, child.Node.Repeating ? new FerruleRepeated(items) : items.FirstOrDefault() ?? new FerruleScalar(FerruleValue.Null)));
            }
            return new FerruleGroup(children);
        }
    }
}
