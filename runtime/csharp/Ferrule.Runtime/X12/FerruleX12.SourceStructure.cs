using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    private sealed record NativeParticle(Node Node, string? Segment, uint Minimum, uint? Maximum, bool RespectMaximum, uint MergedEntries, NativeParticle[] Children)
    {
        internal static NativeParticle Read(JsonElement value, Node node, int depth, ref int count)
        {
            if (depth > 64 || ++count > 10000 || value.ValueKind != JsonValueKind.Object) throw Failure(FerruleX12Error.Schema, "Invalid native X12 structure.");
            string[] names = value.EnumerateObject().Select(p => p.Name).ToArray();
            if (names.Length != 7 || names.Distinct(StringComparer.Ordinal).Count() != 7 || names.Any(n => n is not ("name" or "segment" or "minimum" or "maximum" or "respect_maximum" or "merged_entries" or "children"))) throw Failure(FerruleX12Error.Schema, "Invalid native X12 structure fields.");
            var name = value.GetProperty("name"); var segment = value.GetProperty("segment"); var minimum = value.GetProperty("minimum"); var maximum = value.GetProperty("maximum"); var respect = value.GetProperty("respect_maximum"); var merged = value.GetProperty("merged_entries"); var children = value.GetProperty("children");
            if (name.ValueKind != JsonValueKind.String || name.GetString() != node.Name || node.Type is not null || minimum.ValueKind != JsonValueKind.Number || !minimum.TryGetUInt32(out uint min) || min > 1024 || merged.ValueKind != JsonValueKind.Number || !merged.TryGetUInt32(out uint entries) || entries is 0 or > 1024 || respect.ValueKind is not (JsonValueKind.True or JsonValueKind.False) || children.ValueKind != JsonValueKind.Array) throw Failure(FerruleX12Error.Schema, "Invalid native X12 structure binding.");
            uint? max = null;
            if (maximum.ValueKind != JsonValueKind.Null) { if (maximum.ValueKind != JsonValueKind.Number || !maximum.TryGetUInt32(out uint finite) || finite < min) throw Failure(FerruleX12Error.Schema, "Invalid native X12 maximum."); max = finite; }
            string? id = null;
            if (segment.ValueKind != JsonValueKind.Null) { if (segment.ValueKind != JsonValueKind.String) throw Failure(FerruleX12Error.Schema, "Invalid native segment."); id = segment.GetString()!; if (id.Length is < 2 or > 3 || id.Any(c => !(char.IsAsciiLetterUpper(c) || char.IsAsciiDigit(c))) || children.GetArrayLength() != 0) throw Failure(FerruleX12Error.Schema, "Invalid native segment."); }
            var childPlans = new List<NativeParticle>();
            if (id is null)
            {
                if (children.GetArrayLength() != node.Children.Length) throw Failure(FerruleX12Error.Schema, "Native X12 structure does not cover its schema.");
                int index = 0;
                foreach (var child in children.EnumerateArray()) childPlans.Add(Read(child, node.Children[index++], depth + 1, ref count));
            }
            return new(node, id, min, max, respect.GetBoolean(), entries, childPlans.ToArray());
        }
    }

    private static FerruleInstance ParseNativeStructureSource(Profile profile, JsonElement structure, byte[] bytes, string encoding)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        if (bytes.Length > MaximumDocumentBytes) throw Limit("X12 source byte limit exceeded.");
        bool bom = bytes.AsSpan().StartsWith(new byte[] { 0xef, 0xbb, 0xbf }) || bytes.AsSpan().StartsWith(new byte[] { 0xff, 0xfe }) || bytes.AsSpan().StartsWith(new byte[] { 0xfe, 0xff }) || bytes.AsSpan().StartsWith(new byte[] { 0, 0, 0xfe, 0xff });
        string text = encoding == "latin1" && !bom ? Encoding.Latin1.GetString(bytes) : FerruleTextDecoding.BomAwareNonFlushing(bytes);
        string first = text.TrimStart();
        char terminator = first.Length > 105 ? first[105] : '\0';
        text = new string(text.Where(c => c is not ('\t' or '\r' or '\n') || c == terminator).ToArray());
        RequireUtf8(text, MaximumDocumentBytes, "X12 source");
        int count = 0;
        var plan = NativeParticle.Read(structure, LexicalNode(profile.Root), 0, ref count);
        var reader = new NativeSourceReader(text, profile);
        reader.Skip();
        if (reader.Remaining.StartsWith("ISA", StringComparison.Ordinal)) reader.Discover();
        return reader.Read(plan, 0);
    }

    private sealed class NativeSourceReader(string text, Profile profile)
    {
        private int position;
        private int nodes;
        private int segments;
        private Syntax syntax = new('*', ':', '~');
        internal string Remaining => text[position..];
        internal void Skip() { while (position < text.Length && char.IsWhiteSpace(text[position])) position++; }
        private string Tag()
        {
            string remaining = Remaining.TrimStart();
            if (remaining.StartsWith("ISA", StringComparison.Ordinal)) return "ISA";
            int end = remaining.IndexOfAny([syntax.Element, syntax.Component, syntax.Segment]);
            return end < 0 ? remaining : remaining[..end];
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
        private bool Matches(NativeParticle plan)
        {
            if (plan.Segment is { } id)
            {
                if (Tag() != id) return false;
                if (id == "ISA") return true;
                string[] fields = Remaining.TrimStart().Split(syntax.Segment, 2)[0].Split(syntax.Element, plan.Node.Children.Length + 2).Skip(1).ToArray();
                for (int i = 0; i < plan.Node.Children.Length; i++) if (plan.Node.Children[i].Fixed is { } fixedValue && (i < fields.Length ? fields[i] : "") != fixedValue) return false;
                return true;
            }
            foreach (var child in plan.Children) { if (Matches(child)) return true; if (child.Minimum > 0) break; }
            return false;
        }
        internal FerruleInstance Read(NativeParticle plan, int depth)
        {
            if (depth > 64 || ++nodes > MaximumNodes) throw Limit("Native X12 structure limit exceeded.");
            if (plan.Segment is { } id)
            {
                Skip(); if (id == "ISA") Discover();
                int end = Remaining.IndexOf(syntax.Segment);
                if (end < 0) throw Failure(FerruleX12Error.Syntax, "Unterminated native X12 segment.");
                string rawSegment = Remaining[..end];
                if (rawSegment.Count(c => c == syntax.Element) > MaximumElements) throw Limit("Native X12 element limit exceeded.");
                string[] elements = rawSegment.Split(syntax.Element).Skip(1).ToArray();
                int tokens = elements.Length;
                foreach (string rawElement in elements)
                {
                    int components = id == "ISA" ? 1 : rawElement.Count(c => c == syntax.Component) + 1;
                    int repeats = id != "ISA" && syntax.Repetition is { } repetition ? rawElement.Count(c => c == repetition) + 1 : 1;
                    if (components > MaximumElements || repeats > MaximumLoopInstances || tokens > MaximumNodes - components - repeats) throw Limit("Native X12 token limit exceeded.");
                    tokens += components + repeats;
                }
                if (nodes > MaximumNodes - tokens) throw Limit("Native X12 token limit exceeded.");
                nodes += tokens;
                var cursor = new Cursor([], syntax, profile, lexical: true);
                var fields = new List<FerruleField>();
                for (int i = 0; i < plan.Node.Children.Length; i++)
                {
                    Node node = plan.Node.Children[i]; string? raw = i < elements.Length ? elements[i] : null;
                    fields.Add(new(node.Name, node.Repeating ? ReadRepeatedElement(node, raw, cursor, [plan.Node.Name, node.Name], depth + 1) : ReadElement(node, raw ?? "", cursor, [plan.Node.Name, node.Name], depth + 1, id == "ISA" && i == 15, raw is not null)));
                }
                position += end + 1; if (++segments > MaximumSegments) throw Limit("Native X12 segment limit exceeded.");
                return new FerruleGroup(fields);
            }
            var children = new List<FerruleField>();
            foreach (var child in plan.Children)
            {
                uint? maximum = child.MergedEntries > 1 ? child.MergedEntries : child.RespectMaximum || child.Maximum is <= 1 ? child.Maximum : null;
                var items = new List<FerruleInstance>();
                while (maximum is null || items.Count < maximum.Value)
                {
                    Skip(); if (position == text.Length || !Matches(child)) break;
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
