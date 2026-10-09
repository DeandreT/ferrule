using System.Globalization;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>
/// Optional package-free, strict UTF-8 004010 interchange parsing and writing.
/// Limits bound documents and traversal; they do not promise a process-memory cap.
/// </summary>
public static partial class FerruleX12
{
    public const int MaximumSchemaBytes = 1024 * 1024;
    public const int MaximumDocumentBytes = 64 * 1024 * 1024;
    public const int MaximumSegments = 100_000;
    public const int MaximumDepth = 64;
    public const int MaximumNodes = 1_000_000;
    public const int MaximumLoopInstances = 100_000;
    private const int MaximumElements = 1024;
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private static readonly int[] IsaWidths = [2, 10, 2, 10, 2, 15, 2, 15, 6, 4, 1, 5, 9, 1, 1, 1];

    public static FerruleInstance ParseEmbeddedBytes(string descriptor, byte[] document)
    {
        ArgumentNullException.ThrowIfNull(document);
        if (document.Length > MaximumDocumentBytes) throw Limit("X12 input byte limit exceeded.");
        string text;
        try { text = StrictUtf8.GetString(document); }
        catch (DecoderFallbackException) { throw Failure(FerruleX12Error.Encoding, "X12 input is not strict UTF-8."); }
        return ParseEmbedded(descriptor, text);
    }

    public static FerruleInstance ParseEmbedded(string descriptor, string document)
    {
        ArgumentNullException.ThrowIfNull(document);
        RequireUtf8(document, MaximumDocumentBytes, "X12 input");
        Profile profile = ParseProfile(descriptor);
        Syntax syntax = DiscoverSyntax(document);
        if (profile.Separators is { } selected && selected != syntax)
            throw Failure(FerruleX12Error.Syntax, "Configured separators do not match ISA.");
        List<Segment> segments = Tokenize(document, syntax);
        ValidateEnvelope(segments);
        var cursor = new Cursor(segments, syntax, profile);
        FerruleInstance result = ReadContainer(profile.Root, cursor, [], 0, root: true);
        if (cursor.Position != segments.Count)
            throw Failure(FerruleX12Error.Schema, "The schema did not consume the complete interchange.", cursor.Position);
        return result;
    }

    public static byte[] SerializeEmbeddedBytes(string descriptor, FerruleInstance document)
        => StrictUtf8.GetBytes(SerializeEmbedded(descriptor, document));

    public static string SerializeEmbedded(string descriptor, FerruleInstance document)
    {
        ArgumentNullException.ThrowIfNull(document);
        Profile profile = ParseProfile(descriptor);
        Syntax syntax = profile.Separators ?? new('*', ':', '~');
        var segments = new List<Segment>();
        var budget = new Budget();
        WriteContainer(profile.Root, document, profile, [], segments, budget, 0, root: true);
        // ISA padding is a fixed-width encoding operation, never control allocation.
        if (segments.Count == 0 || segments[0].Id != "ISA" || segments[0].Elements.Length != 16)
            throw Failure(FerruleX12Error.Envelope, "X12 output requires a complete ISA.");
        for (int index = 0; index < 16; index++)
        {
            string value = segments[0].Elements[index];
            if (index is 1 or 3 or 5 or 7) value = value.PadRight(IsaWidths[index]);
            segments[0].Elements[index] = value;
        }
        if (segments[0].Elements[15] != syntax.Component.ToString())
            throw Failure(FerruleX12Error.Syntax, "ISA16 must match the selected component separator.");
        ValidateEnvelope(segments);
        var output = new StringBuilder();
        long bytes = 0;
        foreach (Segment segment in segments)
        {
            int last = segment.Elements.Length;
            if (segment.Id != "ISA") while (last > 0 && segment.Elements[last - 1].Length == 0) last--;
            Add(segment.Id);
            for (int index = 0; index < last; index++)
            {
                string text = segment.Elements[index];
                RequireOutputText(text, syntax, segment.Id == "ISA" && index == 15);
                Add(syntax.Element.ToString());
                Add(text);
            }
            Add(syntax.Segment.ToString());
            // LF is already a segment terminator; other syntax uses one segment per line.
            if (syntax.Segment != '\n') Add("\n");
        }
        return output.ToString();

        void Add(string text)
        {
            try { bytes += StrictUtf8.GetByteCount(text); }
            catch (EncoderFallbackException) { throw Failure(FerruleX12Error.Encoding, "X12 output contains invalid Unicode."); }
            if (bytes > MaximumDocumentBytes) throw Limit("X12 output byte limit exceeded.");
            output.Append(text);
        }
    }

    private readonly record struct Syntax(char Element, char Component, char Segment);
    private sealed record Segment(string Id, string[] Elements);

    private static Syntax DiscoverSyntax(string text)
    {
        // Require the fixed-width ISA exactly at the boundary. A BOM or leading whitespace
        // is not silently removed from the caller's document.
        if (text.Length < 106 || !text.StartsWith("ISA", StringComparison.Ordinal))
            throw Failure(FerruleX12Error.Syntax, "X12 requires a complete fixed-width ISA at offset zero.");
        char element = text[3];
        int offset = 3;
        for (int index = 0; index < 16; index++)
        {
            if (text[offset] != element) throw Failure(FerruleX12Error.Envelope, "ISA element widths are invalid.", 0);
            offset += 1 + IsaWidths[index];
        }
        var syntax = new Syntax(element, text[104], text[105]);
        ValidateSyntax(syntax);
        return syntax;
    }

    private static void ValidateSyntax(Syntax syntax)
    {
        static bool Visible(char value) => value is >= '!' and <= '~' && !char.IsAsciiLetterOrDigit(value);
        if (!Visible(syntax.Element) || !Visible(syntax.Component)
            || !(Visible(syntax.Segment) || syntax.Segment == '\n')
            || syntax.Element == syntax.Component || syntax.Element == syntax.Segment || syntax.Component == syntax.Segment)
            throw Failure(FerruleX12Error.Syntax, "X12 separators must be distinct ASCII punctuation; the segment terminator may be LF.");
    }

    private static List<Segment> Tokenize(string text, Syntax syntax)
    {
        var segments = new List<Segment>();
        int start = 0;
        while (start < text.Length)
        {
            // Formatting is allowed between completed segments, never inside elements.
            if (start > 0) while (start < text.Length && text[start] is '\r' or '\n' or ' ' or '\t') start++;
            if (start == text.Length) break;
            int end = text.IndexOf(syntax.Segment, start);
            if (end < 0) throw Failure(FerruleX12Error.Syntax, "X12 has an unterminated segment.", segments.Count);
            if (segments.Count == MaximumSegments) throw Limit("X12 segment limit exceeded.");
            var parts = new List<string>();
            int position = start;
            int componentSeparators = 0;
            for (int index = start; index <= end; index++)
            {
                if (index == end || text[index] == syntax.Element)
                {
                    if (parts.Count > MaximumElements) throw Limit("X12 element limit exceeded.");
                    parts.Add(text[position..index]);
                    position = index + 1;
                    componentSeparators = 0;
                }
                else if (char.IsControl(text[index]))
                    throw Failure(FerruleX12Error.Syntax, "X12 contains a control character inside a segment.", segments.Count);
                else if (text[index] == syntax.Component && ++componentSeparators >= MaximumElements)
                    throw Limit("X12 component limit exceeded.");
            }
            string id = parts[0];
            if (id.Length is < 2 or > 3 || !char.IsAsciiLetterUpper(id[0])
                || id.Any(character => !char.IsAsciiLetterUpper(character) && !char.IsAsciiDigit(character)))
                throw Failure(FerruleX12Error.Syntax, "X12 segment identifiers must be two or three uppercase alphanumeric characters.", segments.Count);
            segments.Add(new(id, parts.Skip(1).ToArray()));
            start = end + 1;
        }
        return segments;
    }

    private static void ValidateEnvelope(IReadOnlyList<Segment> segments)
    {
        string[] ids = ["ISA", "GS", "ST", "SE", "GE", "IEA"];
        var indexes = new int[6];
        for (int index = 0; index < ids.Length; index++)
        {
            int[] matches = segments.Select((segment, at) => (segment, at))
                .Where(pair => pair.segment.Id == ids[index]).Select(pair => pair.at).ToArray();
            if (matches.Length != 1) throw Failure(FerruleX12Error.Envelope, "The profile requires one interchange, group and transaction.");
            indexes[index] = matches[0];
        }
        if (indexes[0] != 0 || indexes[1] != 1 || indexes[2] != 2
            || indexes[3] != segments.Count - 3 || indexes[4] != segments.Count - 2 || indexes[5] != segments.Count - 1)
            throw Failure(FerruleX12Error.Envelope, "X12 envelope ordering is invalid.");
        Segment isa = segments[0], gs = segments[1], st = segments[2];
        Segment se = segments[^3], ge = segments[^2], iea = segments[^1];
        if (isa.Elements.Length != 16 || gs.Elements.Length != 8 || st.Elements.Length != 2
            || se.Elements.Length != 2 || ge.Elements.Length != 2 || iea.Elements.Length != 2)
            throw Failure(FerruleX12Error.Envelope, "X12 envelope element counts are invalid.");
        for (int index = 0; index < 16; index++)
            if (isa.Elements[index].Length != IsaWidths[index] || isa.Elements[index].Any(character => character > 127))
                throw Failure(FerruleX12Error.Envelope, "ISA requires its declared ASCII widths.", 0);
        if (isa.Elements[11] != "00401" || gs.Elements[7] != "004010" || isa.Elements[10] != "U")
            throw Failure(FerruleX12Error.UnsupportedProfile, "Only the 00401/004010 envelope without element repetition is supported.");
        if (!Digits(isa.Elements[12], 9, 9) || !Digits(gs.Elements[5], 1, 9) || !Digits(st.Elements[1], 4, 9))
            throw Failure(FerruleX12Error.Envelope, "X12 control numbers have invalid lexical shapes.");
        if (!Digits(st.Elements[0], 3, 3) || string.IsNullOrWhiteSpace(gs.Elements[0])
            || string.IsNullOrWhiteSpace(isa.Elements[5]) || string.IsNullOrWhiteSpace(isa.Elements[7])
            || string.IsNullOrWhiteSpace(gs.Elements[1]) || string.IsNullOrWhiteSpace(gs.Elements[2]))
            throw Failure(FerruleX12Error.Envelope, "X12 requires sender, recipient, transaction and functional group identities.");
        if (se.Elements[1] != st.Elements[1] || ge.Elements[1] != gs.Elements[5] || iea.Elements[1] != isa.Elements[12]
            || !Count(se.Elements[0], segments.Count - 4) || !Count(ge.Elements[0], 1) || !Count(iea.Elements[0], 1))
            throw Failure(FerruleX12Error.Envelope, "X12 trailer counts or controls do not match the interchange.");
        if (!Digits(isa.Elements[8], 6, 6) || !Digits(isa.Elements[9], 4, 4)
            || !Digits(gs.Elements[3], 8, 8) || !Digits(gs.Elements[4], 4, 8)
            || isa.Elements[13] is not ("0" or "1") || isa.Elements[14] is not ("P" or "T" or "I"))
            throw Failure(FerruleX12Error.Envelope, "X12 envelope date, time or indicator shapes are invalid.");
        if (!DateOnly.TryParseExact("20" + isa.Elements[8], "yyyyMMdd", CultureInfo.InvariantCulture,
                DateTimeStyles.None, out _) || !DateOnly.TryParseExact(gs.Elements[3], "yyyyMMdd", CultureInfo.InvariantCulture,
                DateTimeStyles.None, out _) || !Clock(isa.Elements[9]) || !Clock(gs.Elements[4]))
            throw Failure(FerruleX12Error.Envelope, "X12 envelope dates or times are invalid.");

        static bool Count(string value, int expected) => Digits(value, 1, 9)
            && int.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out int count) && count == expected;
        static bool Clock(string value) => value.Length is 4 or 6 or 7 or 8
            && int.Parse(value[..2], CultureInfo.InvariantCulture) < 24
            && int.Parse(value[2..4], CultureInfo.InvariantCulture) < 60
            && (value.Length == 4 || int.Parse(value[4..6], CultureInfo.InvariantCulture) < 60);
    }

    private static bool Digits(string text, int minimum, int maximum)
        => text.Length >= minimum && text.Length <= maximum && text.All(char.IsAsciiDigit);

    private static void RequireOutputText(string text, Syntax syntax, bool isaComponent)
    {
        if (isaComponent && text == syntax.Component.ToString()) return;
        if (text.Any(character => char.IsControl(character) || character == syntax.Element || character == syntax.Segment))
            throw Failure(FerruleX12Error.Value, "X12 output contains an unrepresentable delimiter or control character.");
        // Composite elements have already been built from validated components.
    }

    private static void RequireUtf8(string text, int maximum, string label)
    {
        try { if (StrictUtf8.GetByteCount(text) > maximum) throw Limit(label + " byte limit exceeded."); }
        catch (EncoderFallbackException) { throw Failure(FerruleX12Error.Encoding, label + " contains invalid Unicode."); }
    }

    private static FerruleX12Exception Failure(FerruleX12Error error, string message, int? segment = null, string? path = null)
        => new(error, message, segment, path);
    private static FerruleX12Exception Limit(string message) => Failure(FerruleX12Error.ResourceLimit, message);

    private sealed class Budget
    {
        private int nodes;
        private int loops;
        private long outputBytes;
        internal long RemainingOutput => MaximumDocumentBytes - outputBytes;
        internal void Visit(int depth)
        {
            if (depth > MaximumDepth || ++nodes > MaximumNodes) throw Limit("X12 traversal limit exceeded.");
        }
        internal void Loop()
        {
            if (++loops > MaximumLoopInstances) throw Limit("X12 loop instance limit exceeded.");
        }
        internal void CheckOutput(long segmentBytes)
        {
            if (outputBytes + segmentBytes > MaximumDocumentBytes) throw Limit("X12 output byte limit exceeded.");
        }
        internal void AddOutput(long segmentBytes)
        {
            CheckOutput(segmentBytes);
            outputBytes += segmentBytes;
        }
    }
}
