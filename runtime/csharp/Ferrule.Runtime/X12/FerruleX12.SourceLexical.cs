using System.Text;

namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    /// <summary>Tolerant native source value projection. It does not perform
    /// numeric conversions, calendar validation, file acquisition or publication.</summary>
    public static FerruleInstance ParseMapForceSourceBytes(string descriptor, byte[] bytes)
    {
        Profile profile = MapForceSourceProfile(descriptor);
        FerruleInstance lexical = ParseLexicalSourceCore(descriptor, bytes, nativeProjection: true);
        int textBytes = 0;
        return ProjectMapForceSource(profile.Root, lexical, positional: false, segment: false, new Budget(), ref textBytes, 0);
    }

    internal static void ValidateMapForceSourceDescriptor(string descriptor)
        => _ = MapForceSourceProfile(descriptor);

    private static Profile MapForceSourceProfile(string descriptor)
    {
        Profile profile = ParseProfile(descriptor);
        if (profile.MapForce is null || HasFractionalImplied(profile.Root))
            throw Failure(FerruleX12Error.UnsupportedProfile, "MapForce source projection requires a MapForce envelope without fractional implied decimals.");
        return profile;
    }

    /// <summary>Call within a loader before input cleanup; no file is opened here.</summary>
    public static FerruleParsedX12Source ParseMapForceSourceDocument(string descriptor, string identity, byte[] bytes)
        => new(descriptor, new FerruleDocument(identity, ParseMapForceSourceBytes(descriptor, bytes)));

    private static bool HasFractionalImplied(Node node) => node.ImpliedPlaces is > 0 || node.Children.Any(HasFractionalImplied);

    private static FerruleInstance ProjectMapForceSource(Node node, FerruleInstance value, bool positional, bool segment, Budget budget, ref int textBytes, int depth)
    {
        budget.Visit(depth);
        if (value is FerruleRepeated repeated)
        {
            var items = new List<FerruleInstance>();
            foreach (FerruleInstance item in repeated.Items) { budget.Loop(); items.Add(ProjectMapForceSource(node, item, positional, segment, budget, ref textBytes, depth + 1)); }
            return new FerruleRepeated(items);
        }
        if (value is FerruleScalar scalar)
        {
            if (scalar.Value.Kind == FerruleValueKind.Null) return scalar;
            if (scalar.Value.Kind != FerruleValueKind.String) throw Failure(FerruleX12Error.Schema, "Unsupported lexical source value.");
            string text = scalar.Value.StringValue;
            if (text.Length == 0) return new FerruleScalar(FerruleValue.Null);
            text = node.Lexical?.Kind switch { "compact_date6" or "compact_date8" => MapForceSourceDate(text), "compact_time" => MapForceSourceTime(text), "decimal" => MapForceSourceNumber(text), _ => text };
            int bytes = Encoding.UTF8.GetByteCount(text);
            if (bytes > MaximumDocumentBytes - textBytes) throw Limit("X12 source projected text limit exceeded.");
            textBytes += bytes;
            return new FerruleScalar(FerruleValue.FromString(text));
        }
        if (value is not FerruleGroup group) throw Failure(FerruleX12Error.Schema, "Unsupported lexical source value.");
        var fields = new List<FerruleField>();
        foreach (Node child in node.Children)
        {
            FerruleField? field = group.Fields.FirstOrDefault(field => field.Name == child.Name);
            if (field is null) throw Failure(FerruleX12Error.Schema, "Lexical source field is missing from its schema-shaped view.");
            budget.Name(child.Name);
            bool childPositional = positional || segment;
            bool childSegment = !childPositional && child.Type is null && SegmentId(child.Name) is not null;
            fields.Add(new FerruleField(child.Name, ProjectMapForceSource(child, field.Value, childPositional, childSegment, budget, ref textBytes, depth + 1)));
        }
        if (positional && fields.All(field => field.Value is FerruleScalar item && item.Value.Kind == FerruleValueKind.Null)) return new FerruleScalar(FerruleValue.Null);
        return group.CloneFields(fields);
    }

    private static string MapForceSourceDate(string text)
    {
        text = text.TrimStart();
        if (text.Length is not (6 or 8) || !text.All(char.IsAsciiDigit)) return text;
        if (text.Length == 6) text = (string.CompareOrdinal(text[..2], "50") < 0 ? "20" : "19") + text;
        return text[..4] + "-" + text[4..6] + "-" + text[6..];
    }

    private static string MapForceSourceTime(string text)
    {
        text = text.TrimStart();
        if (text.Length < 4 || text.Length == 5) return text;
        if (text.Length == 4) text += "00";
        if (text.Length > 6) text = text[..6] + "." + text[6..];
        text = text[..4] + ":" + text[4..];
        text = text[..2] + ":" + text[2..];
        return Encoding.UTF8.GetString(Encoding.UTF8.GetBytes(text));
    }

    private static string MapForceSourceNumber(string input)
    {
        string text = input.Trim();
        if (text.Length == 0) return input;
        bool negative = text[0] == '-';
        if (text[0] is '+' or '-') text = text[1..];
        if (text.Length == 0) return "";
        int separator = text.LastIndexOf('.');
        if (separator < 0) separator = text.LastIndexOf(',');
        string integer = separator < 0 ? text : text[..separator];
        string fraction = separator < 0 ? "" : text[(separator + 1)..];
        integer = integer.Length == 0 ? "0" : integer.Replace(".", "", StringComparison.Ordinal).Replace(",", "", StringComparison.Ordinal);
        if (!integer.All(char.IsAsciiDigit) || !fraction.All(char.IsAsciiDigit)) return text;
        return (negative ? "-" : "") + integer + (fraction.Length == 0 ? "" : "." + fraction);
    }

    /// <summary>Explicit structural/lexical source view; strict typed Parse is unchanged.
    /// Scalar validation and conversion remain the mapping's reached operations.</summary>
    public static FerruleInstance ParseLexicalSourceBytes(string descriptor, byte[] bytes)
        => ParseLexicalSourceCore(descriptor, bytes, nativeProjection: false);

    private static FerruleInstance ParseLexicalSourceCore(string descriptor, byte[] bytes, bool nativeProjection)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        if (bytes.Length > MaximumDocumentBytes) throw Limit("X12 source byte limit exceeded.");
        string text = FerruleTextDecoding.BomAwareNonFlushing(bytes);
        if (nativeProjection)
        {
            char terminator = text.Length > 105 ? text[105] : '\0';
            text = new string(text.Where(ch => ch is not ('\t' or '\r' or '\n') || ch == terminator).ToArray());
        }
        RequireUtf8(text, MaximumDocumentBytes, "X12 source");
        Profile original = ParseProfile(descriptor);
        if (text.Length < 106) throw Failure(FerruleX12Error.Syntax, "X12 requires an ISA header.");
        string version = text.Substring(84, 5);
        Node root = LexicalNode(original.Root);
        Profile profile = original with { Root = root, Version = new(version, version + "0"),
            Separators = null, ConfiguredRepetition = null, LenientSegments = false,
            MapForce = original.MapForce is null ? null : ValidateMapForceShape(root, input: true) };
        Syntax syntax = DiscoverSyntax(text, profile);
        List<Segment> segments = Tokenize(text, syntax, new HashSet<string>(RepeatingSegmentIds(root, root: true), StringComparer.Ordinal), allowControls: nativeProjection);
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

public sealed class FerruleParsedX12Source
{
    internal FerruleParsedX12Source(string descriptor, FerruleDocument document) { Descriptor = descriptor; Document = document; }
    internal string Descriptor { get; }
    internal FerruleDocument Document { get; }
}

public enum FerruleX12AcquisitionStage { Filename, Selection, Request, Load, Limit, Descriptor }

public sealed class FerruleX12AcquisitionException : Exception
{
    internal FerruleX12AcquisitionException(FerruleX12AcquisitionStage stage, string message, Exception? cause = null, string? path = null) : base(message, cause) { Stage = stage; Path = path; }
    public FerruleX12AcquisitionStage Stage { get; }
    public string? Path { get; }
}

/// <summary>Lazy, descriptor-bound acquisition. Loaders parse and finish input cleanup before returning.</summary>
public static class FerruleX12SourceAcquisition
{
    public const int MaximumDocuments = 100_000;
    public static IEnumerable<FerruleDocument> Acquire(string descriptor, Func<string> filename, Func<string, IEnumerable<string>> select, Func<string, FerruleParsedX12Source> load, int maximum)
    {
        ArgumentNullException.ThrowIfNull(filename); ArgumentNullException.ThrowIfNull(select); ArgumentNullException.ThrowIfNull(load);
        FerruleX12.ValidateMapForceSourceDescriptor(descriptor);
        if (maximum < 0 || maximum > MaximumDocuments) throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Limit, "Invalid X12 source document limit.");
        return Iterate();
        IEnumerable<FerruleDocument> Iterate()
        {
            string pattern;
            try { pattern = filename(); }
            catch (Exception error) { throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Filename, "X12 source filename failed.", error); }
            IEnumerator<string> requests;
            try { requests = (select(pattern) ?? throw new InvalidOperationException("Null X12 selection.")).GetEnumerator(); }
            catch (Exception error) { throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Selection, "X12 source selection failed.", error); }
            using (requests)
            {
                int count = 0;
                while (true)
                {
                    string path; bool next;
                    try { next = requests.MoveNext(); path = next ? requests.Current : ""; }
                    catch (Exception error) { throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Request, "X12 source request failed.", error); }
                    if (!next) yield break;
                    if (count == maximum) throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Limit, "X12 source document limit reached.");
                    FerruleParsedX12Source source;
                    try { source = load(path) ?? throw new InvalidOperationException("Null X12 source document."); }
                    catch (Exception error) { throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Load, "X12 source load failed.", error, path); }
                    if (source.Descriptor != descriptor) throw new FerruleX12AcquisitionException(FerruleX12AcquisitionStage.Descriptor, "X12 source used a different descriptor.", path: path);
                    count++;
                    yield return source.Document;
                }
            }
        }
    }
}
