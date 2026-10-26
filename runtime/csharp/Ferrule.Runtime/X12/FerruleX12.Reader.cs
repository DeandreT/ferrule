using System.Globalization;

namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    private sealed class Cursor(List<Segment> segments, Syntax syntax, Profile profile)
    {
        internal List<Segment> Segments { get; } = segments;
        internal Syntax Syntax => Position < Segments.Count ? Segments[Position].ReadSyntax ?? syntax : syntax;
        internal Profile Profile { get; } = profile;
        internal HashSet<string> DeclaredIds { get; } = new(DeclaredSegmentIds(profile.Root, root: true), StringComparer.Ordinal);
        internal int Position { get; set; }
        internal int Loops { get; set; }
        internal Budget Budget { get; } = new();
    }

    private static IEnumerable<string> DeclaredSegmentIds(Node node, bool root = false)
    {
        if (!root && SegmentId(node.Name) is { } id) { yield return id; yield break; }
        foreach (Node child in node.Children)
            foreach (string childId in DeclaredSegmentIds(child)) yield return childId;
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

    private static void SkipUnmatched(Cursor cursor, Node[] expectations)
    {
        while (cursor.Position < cursor.Segments.Count)
        {
            Segment segment = cursor.Segments[cursor.Position];
            // A known segment ID with a malformed qualifier remains declared
            // data. Matches selects its owner; leniency never discards it.
            if (cursor.DeclaredIds.Contains(segment.Id)
                || expectations.Any(trigger => SegmentId(trigger.Name) == segment.Id)) return;
            cursor.Position++;
        }
    }

    private static FerruleInstance ReadContainer(Node node, Cursor cursor, string[] path, int depth, Node[] follow, bool root = false)
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
            {
                Node element = node.Children[index];
                string? raw = index < segment.Elements.Length ? segment.Elements[index] : null;
                string[] elementPath = [.. path, element.Name];
                if (element.Repeating)
                {
                    fields.Add(new(element.Name, ReadRepeatedElement(element, raw, cursor, elementPath, depth + 1)));
                    continue;
                }
                if (raw is not null && segment.Id != "ISA" && cursor.Syntax.Repetition is { } repetition && raw.Contains(repetition))
                    throw Failure(FerruleX12Error.UnsupportedProfile,
                        "Repeated X12 element values are outside the selected profile.", cursor.Position);
                fields.Add(new(element.Name, ReadElement(element, raw ?? "", cursor, elementPath, depth + 1,
                    segment.Id == "ISA" && index == 15)));
            }
            cursor.Position++;
            return new FerruleGroup(fields);
        }
        var children = new List<FerruleField>();
        for (int index = 0; index < node.Children.Length; index++)
        {
            Node child = node.Children[index];
            Node[] triggers = Triggers(child).ToArray();
            Node[] childFollow = cursor.Profile.LenientSegments
                ? [.. node.Children.Skip(index + 1).SelectMany(Triggers), .. follow]
                : [];
            Node[] expectations = cursor.Profile.LenientSegments ? [.. triggers, .. childFollow] : [];
            string[] childPath = [.. path, child.Name];
            if (child.Repeating)
            {
                // All possible starts of the next iteration remain expected
                // while reading its optional prefixes and mandatory content.
                Node[] nestedFollow = cursor.Profile.LenientSegments ? [.. triggers, .. childFollow] : [];
                var items = new List<FerruleInstance>();
                while (true)
                {
                    if (cursor.Profile.LenientSegments) SkipUnmatched(cursor, expectations);
                    if (cursor.Position >= cursor.Segments.Count
                        || !triggers.Any(trigger => Matches(trigger, cursor.Segments[cursor.Position], cursor.Syntax))) break;
                    if (++cursor.Loops > MaximumLoopInstances) throw Limit("X12 loop instance limit exceeded.");
                    int before = cursor.Position;
                    string[] itemPath = cursor.Profile.IndexedPaths
                        ? [.. path, child.Name + "[" + items.Count.ToString(CultureInfo.InvariantCulture) + "]"] : childPath;
                    // Source acknowledgment branches always read as empty collections.
                    if (cursor.Profile.MapForce is { } shape
                        && (ReferenceEquals(child, shape.GroupAcknowledgment) || ReferenceEquals(child, shape.MessageAcknowledgment)))
                        throw AcknowledgmentFailure(cursor.Position, string.Join('/', childPath));
                    items.Add(ReadContainer(child, cursor, itemPath, depth + 1, nestedFollow));
                    if (before == cursor.Position) throw Failure(FerruleX12Error.Schema, "An X12 loop failed to consume a segment.", cursor.Position);
                }
                ValidateCount(child, items.Count, childPath);
                children.Add(new(child.Name, new FerruleRepeated(items)));
            }
            else
            {
                if (cursor.Profile.LenientSegments) SkipUnmatched(cursor, expectations);
                children.Add(new(child.Name, ReadContainer(child, cursor, childPath, depth + 1, childFollow)));
            }
        }
        return new FerruleGroup(children);
    }

    // Mirrors native reading: an absent position has no occurrences, while a
    // present element, even when empty, has one occurrence per repetition part.
    private static FerruleRepeated ReadRepeatedElement(Node node, string? raw, Cursor cursor, string[] path, int depth)
    {
        cursor.Budget.Visit(depth);
        var items = new List<FerruleInstance>();
        if (raw is null) return new FerruleRepeated(items);
        string[] parts = raw.Split(cursor.Syntax.Repetition
            ?? throw Failure(FerruleX12Error.UnsupportedProfile, "Repeated X12 element values are outside the selected profile.", cursor.Position));
        foreach (string part in parts)
        {
            if (++cursor.Loops > MaximumLoopInstances) throw Limit("X12 loop instance limit exceeded.");
            string[] itemPath = cursor.Profile.IndexedPaths
                ? [.. path[..^1], path[^1] + "[" + items.Count.ToString(CultureInfo.InvariantCulture) + "]"] : path;
            items.Add(ReadElement(node, part, cursor, itemPath, depth + 1));
        }
        return new FerruleRepeated(items);
    }

    private static FerruleInstance ReadElement(Node node, string raw, Cursor cursor, string[] path, int depth, bool isaComponent = false)
    {
        cursor.Budget.Visit(depth);
        if (node.Type is not null)
        {
            if (!isaComponent && raw.Contains(cursor.Syntax.Component))
                throw Failure(FerruleX12Error.Schema, "X12 scalar contains undeclared composite syntax.", cursor.Position, string.Join('/', path));
            return new FerruleScalar(ApplyImpliedDecimals(node, ReadScalar(node, raw, path), path));
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

}
