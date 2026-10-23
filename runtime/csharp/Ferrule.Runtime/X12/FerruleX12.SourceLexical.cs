namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    /// <summary>Explicit structural/lexical source view; strict typed Parse is unchanged.
    /// Scalar validation and conversion remain the mapping's reached operations.</summary>
    public static FerruleInstance ParseLexicalSourceBytes(string descriptor, byte[] bytes)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        if (bytes.Length > MaximumDocumentBytes) throw Limit("X12 source byte limit exceeded.");
        string text = FerruleTextDecoding.BomAwareNonFlushing(bytes);
        RequireUtf8(text, MaximumDocumentBytes, "X12 source");
        Profile original = ParseProfile(descriptor);
        if (text.Length < 106) throw Failure(FerruleX12Error.Syntax, "X12 requires an ISA header.");
        string version = text.Substring(84, 5);
        Node root = LexicalNode(original.Root);
        Profile profile = original with { Root = root, Version = new(version, version + "0"),
            Separators = null, ConfiguredRepetition = null, LenientSegments = false,
            MapForce = original.MapForce is null ? null : ValidateMapForceShape(root, input: true) };
        Syntax syntax = DiscoverSyntax(text, profile);
        List<Segment> segments = Tokenize(text, syntax, new HashSet<string>(RepeatingSegmentIds(root, root: true), StringComparer.Ordinal));
        var cursor = new Cursor(segments, syntax, profile, lexical: true);
        var value = ReadContainer(root, cursor, [], 0, [], root: true);
        if (cursor.Position != segments.Count) throw Failure(FerruleX12Error.Schema, "Unconsumed source segment.", cursor.Position);
        return value;
    }

    private static Node LexicalNode(Node node) => new()
    {
        Name = node.Name, Type = node.Type is null ? null : "string", Repeating = node.Repeating,
        Fixed = node.Fixed, Count = node.Count, Children = node.Children.Select(LexicalNode).ToArray(),
    };
}
