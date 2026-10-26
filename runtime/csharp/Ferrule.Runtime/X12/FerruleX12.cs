using System.Globalization;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>
/// Optional package-free, strict UTF-8 profile-guided X12 parsing and writing.
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
        Profile profile = PreparedProfile(descriptor, input: true);
        Syntax syntax = DiscoverSyntax(document, profile);
        List<Segment> segments = Tokenize(document, syntax,
            new HashSet<string>(RepeatingSegmentIds(profile.Root, root: true), StringComparer.Ordinal), profile);
        ValidateEnvelope(segments, profile, syntax);
        var cursor = new Cursor(segments, syntax, profile);
        FerruleInstance result = ReadContainer(profile.Root, cursor, [], 0, [], root: true);
        if (cursor.Position != segments.Count)
            throw Failure(FerruleX12Error.Schema, "The schema did not consume the complete interchange.", cursor.Position);
        return result;
    }

    public static byte[] SerializeEmbeddedBytes(string descriptor, FerruleInstance document)
        => EncodeOutput(SerializeOutput(descriptor, document, null));

    public static byte[] SerializeEmbeddedBytes(string descriptor, FerruleInstance document, FerruleExecutionContext executionContext)
    {
        ArgumentNullException.ThrowIfNull(executionContext);
        return EncodeOutput(SerializeOutput(descriptor, document, executionContext));
    }

    public static string SerializeEmbedded(string descriptor, FerruleInstance document)
        => SerializeOutput(descriptor, document, null).Text.ToString();

    public static string SerializeEmbedded(string descriptor, FerruleInstance document, FerruleExecutionContext executionContext)
    {
        ArgumentNullException.ThrowIfNull(executionContext);
        return SerializeOutput(descriptor, document, executionContext).Text.ToString();
    }

    /// <summary>Complete output text and its UTF-8 length, counted while it was written.</summary>
    private readonly record struct CompletedOutput(StringBuilder Text, long Bytes);

    /// <summary>
    /// Encodes completed output directly from its builder into one array of its
    /// counted length, without first materializing the UTF-16 string. Every
    /// appended piece was strictly counted, so the text is well-formed; the
    /// encoder carries a surrogate pair split across two builder chunks.
    /// </summary>
    private static byte[] EncodeOutput(CompletedOutput output)
    {
        var bytes = new byte[output.Bytes];
        var encoder = StrictUtf8.GetEncoder();
        var written = 0;
        foreach (ReadOnlyMemory<char> chunk in output.Text.GetChunks())
            written += encoder.GetBytes(chunk.Span, bytes.AsSpan(written), flush: false);
        written += encoder.GetBytes(ReadOnlySpan<char>.Empty, bytes.AsSpan(written), flush: true);
        if (written != bytes.Length) throw Failure(FerruleX12Error.Encoding, "X12 output encoding changed its counted length.");
        return bytes;
    }

    private static CompletedOutput SerializeOutput(string descriptor, FerruleInstance document, FerruleExecutionContext? executionContext)
    {
        ArgumentNullException.ThrowIfNull(document);
        Profile profile = PreparedProfile(descriptor, input: false);
        Syntax syntax = profile.OutputSyntax;
        FerruleInstance outputView = HasOutputLexical(profile.Root)
            ? FormatOutputView(profile.Root, document, [], new Budget(), 0)
            : document;
        // MapForce interchanges complete the written wire segments, as native completion does.
        if (profile.Autocomplete is not null && profile.MapForce is null)
            outputView = CompleteOutputView(profile, outputView, executionContext);
        var segments = new List<Segment>();
        var budget = new Budget();
        WriteContainer(profile.Root, outputView, profile, [], segments, budget, 0, root: true);
        if (profile.MapForce is not null)
        {
            if (profile.Autocomplete is not null) segments = CompleteMapForceSegments(profile, segments, executionContext);
            EncodeInterchangeControls(segments);
        }
        else
        {
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
            // Native encoding zero-pads an all-digit IEA02 to the ISA13 width.
            foreach (Segment segment in segments)
                if (segment.Id == "IEA" && segment.Elements.Length >= 2 && Digits(segment.Elements[1], 1, 8))
                    segment.Elements[1] = segment.Elements[1].PadLeft(9, '0');
        }
        ValidateEnvelope(segments, profile, syntax);
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
                RequireOutputText(text, syntax, segment.Id == "ISA" && index == 15,
                    segment.Id == "ISA" && index == 10, segment.Repeating?[index] == true);
                Add(syntax.Element.ToString());
                Add(text);
            }
            Add(syntax.Segment.ToString());
            // LF is already a segment terminator; other syntax uses one segment per line.
            if (syntax.Segment != '\n') Add("\n");
        }
        return new CompletedOutput(output, bytes);

        void Add(string text)
        {
            try { bytes += StrictUtf8.GetByteCount(text); }
            catch (EncoderFallbackException) { throw Failure(FerruleX12Error.Encoding, "X12 output contains invalid Unicode."); }
            if (bytes > MaximumDocumentBytes) throw Limit("X12 output byte limit exceeded.");
            output.Append(text);
        }
    }

    private readonly record struct Syntax(char Element, char Component, char Segment, char? Repetition = null);
    // Output segments mark repeating elements, whose occurrences were validated
    // before the selected repetition delimiter joined them.
    private sealed record Segment(string Id, string[] Elements, bool[]? Repeating = null, Syntax? ReadSyntax = null);

    private static Syntax DiscoverSyntax(string text, Profile profile, int start = 0, int segmentIndex = 0, string? fieldPath = null)
    {
        // Require the fixed-width ISA exactly at the boundary. A BOM or leading whitespace
        // is not silently removed from the caller's document.
        if (text.Length - start < 106 || !text.AsSpan(start).StartsWith("ISA", StringComparison.Ordinal))
            throw Failure(FerruleX12Error.Syntax, "X12 requires a complete fixed-width ISA at offset zero.", fieldPath is null ? null : segmentIndex, fieldPath);
        char element = text[start + 3];
        int offset = start + 3;
        for (int index = 0; index < 16; index++)
        {
            if (text[offset] != element) throw Failure(FerruleX12Error.Envelope, "ISA element widths are invalid.", segmentIndex, fieldPath);
            offset += 1 + IsaWidths[index];
        }
        if (profile.SelectedVersion.Modern && text.Substring(start + 84, 5) != profile.SelectedVersion.Interchange)
            throw Failure(FerruleX12Error.UnsupportedProfile, "X12 envelope versions do not match the selected profile.", fieldPath is null ? null : segmentIndex, fieldPath);
        var syntax = new Syntax(element, text[start + 104], text[start + 105], profile.SelectedVersion.Modern ? text[start + 82] : null);
        ValidateSyntax(syntax, fieldPath is null ? null : segmentIndex, fieldPath);
        if (profile.Separators is { } selected && (selected.Element != syntax.Element
            || selected.Component != syntax.Component || selected.Segment != syntax.Segment
            || selected.Repetition is { } repetition && repetition != syntax.Repetition))
            throw Failure(FerruleX12Error.Syntax, "Configured separators do not match ISA.", fieldPath is null ? null : segmentIndex, fieldPath);
        return syntax;
    }

    private static void ValidateSyntax(Syntax syntax, int? segmentIndex = null, string? fieldPath = null)
    {
        static bool Visible(char value) => value is >= '!' and <= '~' && !char.IsAsciiLetterOrDigit(value);
        if (!Visible(syntax.Element) || !Visible(syntax.Component)
            || !(Visible(syntax.Segment) || syntax.Segment == '\n')
            || syntax.Element == syntax.Component || syntax.Element == syntax.Segment || syntax.Component == syntax.Segment
            || syntax.Repetition is { } repetition && (!Visible(repetition) || repetition == syntax.Element
                || repetition == syntax.Component || repetition == syntax.Segment))
            throw Failure(FerruleX12Error.Syntax, "X12 separators must be distinct ASCII punctuation; the segment terminator may be LF.", segmentIndex, fieldPath);
    }

    private static List<Segment> Tokenize(string text, Syntax syntax, HashSet<string> repeatingIds, Profile profile)
    {
        var segments = new List<Segment>();
        int start = 0;
        int interchange = 1;
        while (start < text.Length)
        {
            // Formatting is allowed between completed segments, never inside elements.
            if (start > 0) while (start < text.Length && text[start] is '\r' or '\n' or ' ' or '\t') start++;
            if (start == text.Length) break;
            if (segments.Count != 0 && profile.MapForce is { } shape && text.AsSpan(start).StartsWith("ISA", StringComparison.Ordinal))
            {
                string fieldPath = shape.Interchanges.Name + "[" + interchange.ToString(CultureInfo.InvariantCulture) + "]/" + shape.Isa.Name;
                syntax = DiscoverSyntax(text, profile, start, segments.Count, fieldPath);
                interchange++;
            }
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
            // Segments declaring a repeating element defer this check to their
            // reader, which still refuses repetition in non-repeating elements.
            if (syntax.Repetition is { } repetition && !repeatingIds.Contains(id))
                for (int index = 1; index < parts.Count; index++)
                    if (!(id == "ISA" && index == 11) && parts[index].Contains(repetition))
                        throw Failure(FerruleX12Error.UnsupportedProfile,
                            "Repeated X12 element values are outside the selected profile.", segments.Count);
            segments.Add(new(id, parts.Skip(1).ToArray(), ReadSyntax: syntax));
            start = end + 1;
        }
        return segments;
    }

    // Native encoding of every interchange: space-padded ISA text fields and an
    // all-digit IEA02 zero-padded to the nine-digit ISA13 width.
    private static void EncodeInterchangeControls(List<Segment> segments)
    {
        foreach (Segment segment in segments)
        {
            if (segment.Id == "ISA" && segment.Elements.Length == 16)
                foreach (int index in (int[])[1, 3, 5, 7])
                    segment.Elements[index] = segment.Elements[index].PadRight(IsaWidths[index]);
            if (segment.Id == "IEA" && segment.Elements.Length >= 2 && Digits(segment.Elements[1], 1, 8))
                segment.Elements[1] = segment.Elements[1].PadLeft(9, '0');
        }
    }

    private static void ValidateEnvelope(IReadOnlyList<Segment> segments, Profile profile, Syntax syntax)
    {
        if (profile.MapForce is not null)
        {
            ValidateMapForceEnvelope(segments, profile, syntax);
            return;
        }
        if (profile.Grouped is not null)
        {
            ValidateGroupedEnvelope(segments, profile, syntax);
            return;
        }
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
        if (!profile.SelectedVersion.Modern)
        {
            if (isa.Elements[11] != "00401" || gs.Elements[7] != "004010" || isa.Elements[10] != "U")
                throw Failure(FerruleX12Error.UnsupportedProfile, "Only the 00401/004010 envelope without element repetition is supported.");
        }
        else
        {
            if (isa.Elements[11] != profile.SelectedVersion.Interchange || gs.Elements[7] != profile.SelectedVersion.Group)
                throw Failure(FerruleX12Error.UnsupportedProfile, "X12 envelope versions do not match the selected profile.");
            if (isa.Elements[10] != syntax.Repetition?.ToString())
                throw Failure(FerruleX12Error.Syntax, "ISA11 must match the selected repetition separator.");
        }
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

    private enum GroupedEnvelopeState { GroupOrEnd, TransactionOrEnd, Body, Closed }

    private static void ValidateGroupedEnvelope(IReadOnlyList<Segment> segments, Profile profile, Syntax syntax)
    {
        GroupedShape shape = profile.Grouped!;
        string groupsPath = shape.Groups.Name;
        string groupPath(int index) => groupsPath + "[" + index.ToString(CultureInfo.InvariantCulture) + "]";
        string transactionsPath(int index) => groupPath(index) + "/" + shape.Transactions.Name;
        string transactionPath(int group, int transaction) => transactionsPath(group)
            + "[" + transaction.ToString(CultureInfo.InvariantCulture) + "]";
        static void Width(Segment segment, int count, int index, string path)
        {
            if (segment.Elements.Length != count)
                throw Failure(FerruleX12Error.Envelope, "X12 envelope element counts are invalid.", index, path);
        }
        static void Controls(bool valid, int index, string path)
        {
            if (!valid) throw Failure(FerruleX12Error.Envelope, "X12 control numbers have invalid lexical shapes.", index, path);
        }
        static void Identities(bool valid, int index, string path)
        {
            if (!valid) throw Failure(FerruleX12Error.Envelope,
                "X12 requires sender, recipient, transaction and functional group identities.", index, path);
        }
        static void Dates(bool shapeValid, string date, string time, int index, string path)
        {
            if (!shapeValid) throw Failure(FerruleX12Error.Envelope,
                "X12 envelope date, time or indicator shapes are invalid.", index, path);
            if (!DateOnly.TryParseExact(date, "yyyyMMdd", CultureInfo.InvariantCulture, DateTimeStyles.None, out _)
                || !EnvelopeClock(time))
                throw Failure(FerruleX12Error.Envelope, "X12 envelope dates or times are invalid.", index, path);
        }
        if (segments.Count == 0 || segments[0].Id != "ISA")
            throw Failure(FerruleX12Error.Envelope, "Grouped X12 envelope ordering is invalid.", 0, "ISA");
        Segment isa = segments[0];
        Width(isa, 16, 0, "ISA");
        for (int index = 0; index < 16; index++)
            if (isa.Elements[index].Length != IsaWidths[index] || isa.Elements[index].Any(character => character > 127))
                throw Failure(FerruleX12Error.Envelope, "ISA requires its declared ASCII widths.", 0, "ISA");
        if (isa.Elements[11] != profile.SelectedVersion.Interchange)
            throw Failure(FerruleX12Error.UnsupportedProfile, "X12 envelope versions do not match the selected profile.", 0, "ISA");
        if (profile.SelectedVersion.Modern)
        {
            if (isa.Elements[10] != syntax.Repetition?.ToString())
                throw Failure(FerruleX12Error.Syntax, "ISA11 must match the selected repetition separator.", 0, "ISA");
        }
        else if (isa.Elements[10] != "U")
            throw Failure(FerruleX12Error.UnsupportedProfile, "Only the 00401/004010 envelope without element repetition is supported.", 0, "ISA");
        Controls(Digits(isa.Elements[12], 9, 9), 0, "ISA");
        Identities(!string.IsNullOrWhiteSpace(isa.Elements[5]) && !string.IsNullOrWhiteSpace(isa.Elements[7]), 0, "ISA");
        Dates(Digits(isa.Elements[8], 6, 6) && Digits(isa.Elements[9], 4, 4)
            && (isa.Elements[13] is "0" or "1") && (isa.Elements[14] is "P" or "T" or "I"),
            "20" + isa.Elements[8], isa.Elements[9], 0, "ISA");

        GroupedEnvelopeState state = GroupedEnvelopeState.GroupOrEnd;
        int groupCount = 0, transactionCount = 0, transactionStart = -1;
        Segment? gs = null, st = null;
        for (int index = 1; index < segments.Count; index++)
        {
            Segment segment = segments[index];
            int group = groupCount - 1, transaction = transactionCount - 1;
            if (segment.Id == "ISA")
                throw Failure(FerruleX12Error.UnsupportedProfile, "Multiple X12 interchanges are outside the selected profile.", index, "ISA");
            switch (state)
            {
                case GroupedEnvelopeState.GroupOrEnd:
                    if (segment.Id == "GS")
                    {
                        string path = groupPath(groupCount) + "/GS";
                        Width(segment, 8, index, path);
                        if (segment.Elements[7] != profile.SelectedVersion.Group)
                            throw Failure(FerruleX12Error.UnsupportedProfile,
                                "X12 envelope versions do not match the selected profile.", index, path);
                        Controls(Digits(segment.Elements[5], 1, 9), index, path);
                        Identities(!string.IsNullOrWhiteSpace(segment.Elements[0])
                            && !string.IsNullOrWhiteSpace(segment.Elements[1]) && !string.IsNullOrWhiteSpace(segment.Elements[2]), index, path);
                        Dates(Digits(segment.Elements[3], 8, 8) && Digits(segment.Elements[4], 4, 8),
                            segment.Elements[3], segment.Elements[4], index, path);
                        gs = segment;
                        groupCount++;
                        transactionCount = 0;
                        state = GroupedEnvelopeState.TransactionOrEnd;
                        break;
                    }
                    if (segment.Id == "IEA")
                    {
                        if (groupCount == 0)
                            throw Failure(FerruleX12Error.Envelope,
                                "Grouped X12 interchange requires at least one functional group.", index, groupsPath);
                        Width(segment, 2, index, "IEA");
                        if (!EnvelopeCount(segment.Elements[0], groupCount) || segment.Elements[1] != isa.Elements[12])
                            throw Failure(FerruleX12Error.Envelope,
                                "X12 interchange trailer count or control does not match its owner.", index, "IEA");
                        state = GroupedEnvelopeState.Closed;
                        break;
                    }
                    throw Failure(FerruleX12Error.Envelope, "Grouped X12 envelope ordering is invalid.", index, "IEA");
                case GroupedEnvelopeState.TransactionOrEnd:
                    if (segment.Id == "ST")
                    {
                        string path = transactionPath(group, transactionCount) + "/ST";
                        Width(segment, 2, index, path);
                        Controls(Digits(segment.Elements[1], 4, 9), index, path);
                        Identities(Digits(segment.Elements[0], 3, 3), index, path);
                        st = segment;
                        transactionCount++;
                        transactionStart = index;
                        state = GroupedEnvelopeState.Body;
                        break;
                    }
                    if (segment.Id == "GE")
                    {
                        if (transactionCount == 0)
                            throw Failure(FerruleX12Error.Envelope,
                                "Grouped X12 functional group requires at least one transaction.", index, transactionsPath(group));
                        string path = groupPath(group) + "/GE";
                        Width(segment, 2, index, path);
                        if (!EnvelopeCount(segment.Elements[0], transactionCount) || segment.Elements[1] != gs!.Elements[5])
                            throw Failure(FerruleX12Error.Envelope,
                                "X12 functional group trailer count or control does not match its owner.", index, path);
                        state = GroupedEnvelopeState.GroupOrEnd;
                        break;
                    }
                    throw Failure(FerruleX12Error.Envelope, "Grouped X12 envelope ordering is invalid.", index,
                        segment.Id == "SE" ? transactionsPath(group) : groupPath(group) + "/GE");
                case GroupedEnvelopeState.Body:
                    if (segment.Id == "SE")
                    {
                        string path = transactionPath(group, transaction) + "/SE";
                        Width(segment, 2, index, path);
                        if (!EnvelopeCount(segment.Elements[0], index - transactionStart + 1) || segment.Elements[1] != st!.Elements[1])
                            throw Failure(FerruleX12Error.Envelope,
                                "X12 transaction trailer count or control does not match its owner.", index, path);
                        state = GroupedEnvelopeState.TransactionOrEnd;
                        break;
                    }
                    if (IsEnvelope(segment.Id))
                        throw Failure(FerruleX12Error.Envelope, "Grouped X12 envelope ordering is invalid.", index,
                            transactionPath(group, transaction) + "/SE");
                    break;
                case GroupedEnvelopeState.Closed:
                    throw Failure(FerruleX12Error.Envelope, "Grouped X12 envelope ordering is invalid.", index, "IEA");
            }
        }
        if (state != GroupedEnvelopeState.Closed)
        {
            string path = state switch
            {
                GroupedEnvelopeState.Body => transactionPath(groupCount - 1, transactionCount - 1) + "/SE",
                GroupedEnvelopeState.TransactionOrEnd => groupPath(groupCount - 1) + "/GE",
                _ => "IEA",
            };
            throw Failure(FerruleX12Error.Envelope, "Grouped X12 envelope ordering is invalid.", segments.Count, path);
        }
    }

    private enum InterchangeEnvelopeState { Header, GroupOrTrailer, TransactionOrTrailer, Body }

    // MapForce interchanges: (ISA (GS (ST ... SE)+ GE)+ IEA)+ over physical segments.
    // Numeric controls are compared by value; ST02/SE02 by exact text. Every
    // ISA repeats the selected version and declares its own separator syntax.
    private static void ValidateMapForceEnvelope(IReadOnlyList<Segment> segments, Profile profile, Syntax syntax)
    {
        MapForceShape shape = profile.MapForce!;
        static string Index(int value) => "[" + value.ToString(CultureInfo.InvariantCulture) + "]";
        string interchangePath(int interchange) => shape.Interchanges.Name + Index(interchange);
        string groupPath(int interchange, int group) => interchangePath(interchange) + "/" + shape.Groups.Name + Index(group);
        string messagePath(int interchange, int group, int message) => groupPath(interchange, group) + "/" + shape.Messages.Name + Index(message);
        static void Width(Segment segment, int minimum, int maximum, int index, string path)
        {
            if (segment.Elements.Length < minimum || segment.Elements.Length > maximum)
                throw Failure(FerruleX12Error.Envelope, "X12 envelope element counts are invalid.", index, path);
        }
        static void Controls(bool valid, int index, string path)
        {
            if (!valid) throw Failure(FerruleX12Error.Envelope, "X12 control numbers have invalid lexical shapes.", index, path);
        }
        static void Identities(bool valid, int index, string path)
        {
            if (!valid) throw Failure(FerruleX12Error.Envelope,
                "X12 requires sender, recipient, transaction and functional group identities.", index, path);
        }
        static void Dates(bool shapeValid, string date, string time, int index, string path)
        {
            if (!shapeValid) throw Failure(FerruleX12Error.Envelope,
                "X12 envelope date, time or indicator shapes are invalid.", index, path);
            if (!DateOnly.TryParseExact(date, "yyyyMMdd", CultureInfo.InvariantCulture, DateTimeStyles.None, out _)
                || !EnvelopeClock(time))
                throw Failure(FerruleX12Error.Envelope, "X12 envelope dates or times are invalid.", index, path);
        }
        static bool SameControl(string value, string owner) => Digits(value, 1, 9) && Digits(owner, 1, 9)
            && long.Parse(value, CultureInfo.InvariantCulture) == long.Parse(owner, CultureInfo.InvariantCulture);
        static FerruleX12Exception Ordering(int index, string path)
            => Failure(FerruleX12Error.Envelope, "X12 interchange envelope ordering is invalid.", index, path);
        // A segment that would populate a source acknowledgment branch natively
        // is refused at that branch; acknowledgment content is outside the profile.
        void Acknowledgment(Node? branch, Segment segment, int index, string owner)
        {
            if (branch is not null && Triggers(branch).Any(trigger => Matches(trigger, segment, segment.ReadSyntax ?? syntax)))
                throw AcknowledgmentFailure(index, owner + "/" + branch.Name);
        }

        InterchangeEnvelopeState state = InterchangeEnvelopeState.Header;
        int interchanges = 0, groups = 0, transactions = 0, transactionStart = -1;
        Segment? isa = null, gs = null, st = null;
        for (int index = 0; index < segments.Count; index++)
        {
            Segment segment = segments[index];
            int interchange = interchanges - 1, group = groups - 1, transaction = transactions - 1;
            switch (state)
            {
                case InterchangeEnvelopeState.Header:
                    {
                        string path = interchangePath(interchanges) + "/" + shape.Isa.Name;
                        if (interchanges > 0)
                            Acknowledgment(shape.GroupAcknowledgment, segment, index, interchangePath(interchanges) + "/" + shape.Groups.Name + Index(0));
                        if (segment.Id != "ISA") throw Ordering(index, path);
                        Width(segment, 16, 16, index, path);
                        for (int position = 0; position < 16; position++)
                            if (segment.Elements[position].Length != IsaWidths[position] || segment.Elements[position].Any(character => character > 127))
                                throw Failure(FerruleX12Error.Envelope, "ISA requires its declared ASCII widths.", index, path);
                        if (segment.Elements[11] != profile.SelectedVersion.Interchange)
                            throw Failure(FerruleX12Error.UnsupportedProfile, "X12 envelope versions do not match the selected profile.", index, path);
                        if (profile.SelectedVersion.Modern)
                        {
                            if (segment.Elements[10] != (segment.ReadSyntax ?? syntax).Repetition?.ToString())
                                throw Failure(FerruleX12Error.Syntax, "ISA11 must match the selected repetition separator.", index, path);
                        }
                        else if (segment.Elements[10] != "U")
                            throw Failure(FerruleX12Error.UnsupportedProfile,
                                "Only the 00401/004010 envelope without element repetition is supported.", index, path);
                        if (segment.Elements[15] != (segment.ReadSyntax ?? syntax).Component.ToString())
                            throw Failure(FerruleX12Error.Syntax, "ISA16 must match the selected component separator.", index, path);
                        Controls(Digits(segment.Elements[12], 9, 9), index, path);
                        Identities(!string.IsNullOrWhiteSpace(segment.Elements[5]) && !string.IsNullOrWhiteSpace(segment.Elements[7]), index, path);
                        Dates(Digits(segment.Elements[8], 6, 6) && Digits(segment.Elements[9], 4, 4)
                            && (segment.Elements[13] is "0" or "1") && (segment.Elements[14] is "P" or "T" or "I"),
                            "20" + segment.Elements[8], segment.Elements[9], index, path);
                        isa = segment;
                        interchanges++;
                        groups = 0;
                        state = InterchangeEnvelopeState.GroupOrTrailer;
                        break;
                    }
                case InterchangeEnvelopeState.GroupOrTrailer:
                    if (segment.Id == "GS")
                    {
                        string path = groupPath(interchange, groups) + "/" + shape.Gs.Name;
                        // GS07 and GS08 are unchecked data; the selected version comes from ISA12.
                        Width(segment, 6, 8, index, path);
                        Controls(Digits(segment.Elements[5], 1, 9), index, path);
                        Identities(!string.IsNullOrWhiteSpace(segment.Elements[0])
                            && !string.IsNullOrWhiteSpace(segment.Elements[1]) && !string.IsNullOrWhiteSpace(segment.Elements[2]), index, path);
                        Dates(Digits(segment.Elements[3], 8, 8) && Digits(segment.Elements[4], 4, 8),
                            segment.Elements[3], segment.Elements[4], index, path);
                        gs = segment;
                        groups++;
                        transactions = 0;
                        state = InterchangeEnvelopeState.TransactionOrTrailer;
                        break;
                    }
                    if (segment.Id == "IEA")
                    {
                        if (groups == 0)
                            throw Failure(FerruleX12Error.Envelope, "X12 interchange requires at least one functional group.",
                                index, interchangePath(interchange) + "/" + shape.Groups.Name);
                        string path = interchangePath(interchange) + "/" + shape.Iea.Name;
                        Width(segment, 2, 2, index, path);
                        if (!EnvelopeCount(segment.Elements[0], groups) || !SameControl(segment.Elements[1], isa!.Elements[12]))
                            throw Failure(FerruleX12Error.Envelope,
                                "X12 interchange trailer count or control does not match its owner.", index, path);
                        state = InterchangeEnvelopeState.Header;
                        break;
                    }
                    Acknowledgment(shape.GroupAcknowledgment, segment, index, groupPath(interchange, Math.Max(group, 0)));
                    throw Ordering(index, interchangePath(interchange) + "/" + shape.Iea.Name);
                case InterchangeEnvelopeState.TransactionOrTrailer:
                    if (segment.Id == "ST")
                    {
                        string path = messagePath(interchange, group, transactions) + "/" + shape.St.Name;
                        Width(segment, 2, shape.St.Children.Length, index, path);
                        Controls(Digits(segment.Elements[1], 4, 9), index, path);
                        Identities(Digits(segment.Elements[0], 3, 3), index, path);
                        st = segment;
                        transactions++;
                        transactionStart = index;
                        state = InterchangeEnvelopeState.Body;
                        break;
                    }
                    if (segment.Id == "GE")
                    {
                        if (transactions == 0)
                            throw Failure(FerruleX12Error.Envelope, "X12 functional group requires at least one transaction.",
                                index, groupPath(interchange, group) + "/" + shape.Messages.Name);
                        string path = groupPath(interchange, group) + "/" + shape.Ge.Name;
                        Width(segment, 2, 2, index, path);
                        if (!EnvelopeCount(segment.Elements[0], transactions) || !SameControl(segment.Elements[1], gs!.Elements[5]))
                            throw Failure(FerruleX12Error.Envelope,
                                "X12 functional group trailer count or control does not match its owner.", index, path);
                        state = InterchangeEnvelopeState.GroupOrTrailer;
                        break;
                    }
                    if (transactions > 0)
                        Acknowledgment(shape.MessageAcknowledgment, segment, index, messagePath(interchange, group, transaction));
                    Acknowledgment(shape.GroupAcknowledgment, segment, index, groupPath(interchange, group));
                    throw Ordering(index, segment.Id == "SE"
                        ? groupPath(interchange, group) + "/" + shape.Messages.Name
                        : groupPath(interchange, group) + "/" + shape.Ge.Name);
                case InterchangeEnvelopeState.Body:
                    if (segment.Id == "SE")
                    {
                        string path = messagePath(interchange, group, transaction) + "/" + shape.Se.Name;
                        Width(segment, 2, 2, index, path);
                        if (!EnvelopeCount(segment.Elements[0], index - transactionStart + 1) || segment.Elements[1] != st!.Elements[1])
                            throw Failure(FerruleX12Error.Envelope,
                                "X12 transaction trailer count or control does not match its owner.", index, path);
                        state = InterchangeEnvelopeState.TransactionOrTrailer;
                        break;
                    }
                    if (IsEnvelope(segment.Id))
                        throw Ordering(index, messagePath(interchange, group, transaction) + "/" + shape.Se.Name);
                    break;
            }
        }
        if (state != InterchangeEnvelopeState.Header || interchanges == 0)
            throw Ordering(segments.Count, state switch
            {
                InterchangeEnvelopeState.Body => messagePath(interchanges - 1, groups - 1, transactions - 1) + "/" + shape.Se.Name,
                InterchangeEnvelopeState.TransactionOrTrailer => groupPath(interchanges - 1, groups - 1) + "/" + shape.Ge.Name,
                InterchangeEnvelopeState.GroupOrTrailer => interchangePath(interchanges - 1) + "/" + shape.Iea.Name,
                _ => interchangePath(0) + "/" + shape.Isa.Name,
            });
    }

    private static FerruleX12Exception AcknowledgmentFailure(int index, string path)
        => Failure(FerruleX12Error.Envelope, "X12 acknowledgment segments are outside the MapForce profile.", index, path);

    private static bool EnvelopeCount(string value, int expected) => Digits(value, 1, 9)
        && int.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out int count) && count == expected;

    private static bool EnvelopeClock(string value) => value.Length is 4 or 6 or 7 or 8
        && int.Parse(value[..2], CultureInfo.InvariantCulture) < 24
        && int.Parse(value[2..4], CultureInfo.InvariantCulture) < 60
        && (value.Length == 4 || int.Parse(value[4..6], CultureInfo.InvariantCulture) < 60);

    private static bool Digits(string text, int minimum, int maximum)
        => text.Length >= minimum && text.Length <= maximum && text.All(char.IsAsciiDigit);

    private static void RequireOutputText(string text, Syntax syntax, bool isaComponent, bool isaRepetition, bool repeated = false)
    {
        if (isaComponent && text == syntax.Component.ToString()) return;
        if (isaRepetition && syntax.Repetition is { } repetition && text == repetition.ToString()) return;
        if (text.Any(character => char.IsControl(character) || character == syntax.Element || character == syntax.Segment
            || !repeated && character == syntax.Repetition))
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
