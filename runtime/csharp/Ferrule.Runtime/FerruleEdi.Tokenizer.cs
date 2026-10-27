using System.Text;

namespace Ferrule.Runtime;

public sealed partial class FerruleEdi
{
    private (List<Segment>, Syntax) Tokenize(byte[] bytes)
    {
        if (_kind == "Idoc") return (IdocSegments(bytes), new(' ', ' ', '\n', null, null));
        string text = Utf8.GetString(bytes);
        if (_kind == "Hl7") return Hl7Segments(text);
        if (_kind == "X12") return X12Segments(text.TrimStart());
        if (_kind == "Edifact") return EdifactSegments(text.TrimStart());
        var syntax = new Syntax('+', ':', '\'', '?', null);
        text = text.TrimStart('\ufeff');
        List<Segment> segments = Delimited(text, syntax, true);
        if (segments.Count == 0 || (_kind == "Tradacoms" ? segments[0].Id != "STX" : segments[0].Id is not ("UNB" or "UNH")))
            throw Fail(FerruleEdiError.Format, "Invalid EDI opening segment.");
        return (segments, syntax);
    }

    // Retain release pairs until the innermost component is decoded. This
    // prevents escaped separators from being split a second time.
    private static List<string> Split(string text, char delimiter, char? release)
        => Split(text, new Rune(delimiter), release is { } escaped ? new Rune(escaped) : null);

    private static List<string> Split(string text, Rune delimiter, Rune? release)
    {
        var result = new List<string>();
        int start = 0, at = 0;
        var runes = text.EnumerateRunes().GetEnumerator();
        while (runes.MoveNext())
        {
            Rune rune = runes.Current;
            int position = at;
            at += rune.Utf16SequenceLength;
            if (rune == release)
            {
                if (!runes.MoveNext()) throw Fail(FerruleEdiError.Format, "Dangling EDI release character.");
                at += runes.Current.Utf16SequenceLength;
            }
            else if (rune == delimiter) { result.Add(text[start..position]); start = at; }
        }
        result.Add(text[start..]);
        return result;
    }
    private static string Unescape(string text, char? release)
        => Unescape(text, release is { } escaped ? new Rune(escaped) : null);

    private static string Unescape(string text, Rune? release)
    {
        if (release is null) return text;
        var output = new StringBuilder();
        var runes = text.EnumerateRunes().GetEnumerator();
        while (runes.MoveNext())
        {
            Rune rune = runes.Current;
            if (rune == release)
            {
                if (!runes.MoveNext()) throw Fail(FerruleEdiError.Format, "Dangling EDI release character.");
                rune = runes.Current;
            }
            output.Append(rune.ToString());
        }
        return output.ToString();
    }
    private static List<Segment> Delimited(string text, Syntax syntax, bool assigned, bool edifact = false, Rune? repeatAdvice = null, Rune? elementOverride = null, Rune? componentOverride = null, Rune? terminatorOverride = null, Rune? releaseOverride = null, Rune? repetitionOverride = null)
    {
        var segments = new List<Segment>();
        Rune elementSeparator = elementOverride ?? new Rune(syntax.Element);
        Rune componentSeparator = componentOverride ?? new Rune(syntax.Component);
        Rune terminator = terminatorOverride ?? new Rune(syntax.Terminator);
        Rune? release = releaseOverride ?? (syntax.Release is { } escaped ? new Rune(escaped) : null);
        Rune? repetition = repetitionOverride ?? (syntax.Repetition is { } repeated ? new Rune(repeated) : null);
        Rune advice = repeatAdvice ?? new Rune('*');
        foreach (string raw in Split(text, terminator, release))
        {
            string segment = raw.TrimStart();
            if (segment.Length == 0) continue;
            string id;
            List<string> elements;
            if (assigned)
            {
                var pair = Split(segment, new Rune('='), release);
                if (pair.Count < 2) throw Fail(FerruleEdiError.Format, "TRADACOMS segment has no assignment separator.");
                id = Unescape(pair[0].Trim(), release);
                if (id.Length != 3 || !id.All(char.IsAsciiLetterOrDigit)) throw Fail(FerruleEdiError.Format, "Invalid TRADACOMS segment ID.");
                elements = Split(string.Join('=', pair.Skip(1)), elementSeparator, release);
            }
            else
            {
                elements = Split(segment, elementSeparator, release);
                id = elements[0]; elements.RemoveAt(0);
            }
            var parsed = elements.Select(element => id == "ISA" ? new List<List<string>> { new() { element } } :
                (repetition is { } repeat && !(edifact && id == "UNB") ? Split(element, repeat, release) : [element])
                .Select(value => Split(value, componentSeparator, release).Select(part => Unescape(part, release)).ToList()).ToList()).ToList();
            segments.Add(new(id, parsed));
            if (edifact && id == "UNB")
            {
                string? version = parsed.FirstOrDefault()?.FirstOrDefault()?.ElementAtOrDefault(1);
                bool v4 = IsEdifactV4(version);
                if (v4 && (Rune.IsWhiteSpace(advice) || new Rune?[] { elementSeparator, componentSeparator, terminator, release }.Contains(advice)))
                    throw Fail(FerruleEdiError.Format, "Syntax v4 requires a distinct visible repetition separator.");
                repetition = v4 ? advice : null;
            }
        }
        return segments;
    }

    private static bool IsEdifactV4(string? version, bool fallback = false) =>
        uint.TryParse(version, System.Globalization.NumberStyles.AllowLeadingSign, System.Globalization.CultureInfo.InvariantCulture, out uint number) ? number >= 4 : fallback;

    private static (List<Segment>, Syntax) EdifactSegments(string text)
    {
        var canonical = new Syntax('+', ':', '\'', '?', null);
        var syntax = canonical;
        Rune element = new('+'), component = new(':'), terminator = new('\''), release = new('?'), advice = new('*');
        Rune? repetition = null;
        bool boundary = true, interchange = false, group = false, message = false;
        var segments = new List<Segment>();
        int at = 0;
        while (at < text.Length)
        {
            while (at < text.Length && Rune.IsWhiteSpace(Rune.GetRuneAt(text, at))) at += Rune.GetRuneAt(text, at).Utf16SequenceLength;
            if (at == text.Length) break;
            if (boundary)
            {
                element = new('+'); component = new(':'); terminator = new('\''); release = new('?'); advice = new('*');
                repetition = null;
                bool hasAdvice = text.AsSpan(at).StartsWith("UNA", StringComparison.Ordinal);
                if (hasAdvice)
                {
                    Rune[] service = new Rune[6];
                    int count = 0;
                    foreach (Rune rune in text.AsSpan(at + 3).EnumerateRunes())
                    {
                        service[count++] = rune;
                        if (count == service.Length) break;
                    }
                    if (count < service.Length) throw Fail(FerruleEdiError.Format, "Truncated UNA service string.");
                    component = service[0]; element = service[1]; release = service[3]; advice = service[4]; terminator = service[5];
                    Rune[] active = [component, element, release, terminator];
                    if (active.Any(Rune.IsWhiteSpace) || active.Distinct().Count() != active.Length)
                        throw Fail(FerruleEdiError.Format, "UNA requires distinct visible service characters.");
                    at += 3 + service.Sum(rune => rune.Utf16SequenceLength);
                    while (at < text.Length && Rune.IsWhiteSpace(Rune.GetRuneAt(text, at))) at += Rune.GetRuneAt(text, at).Utf16SequenceLength;
                }
                bool Starts(string id) => text.AsSpan(at).StartsWith(id, StringComparison.Ordinal)
                    && at + id.Length < text.Length && (Rune.GetRuneAt(text, at + id.Length) == element || Rune.GetRuneAt(text, at + id.Length) == terminator);
                if ((segments.Count == 0 || hasAdvice) && !Starts("UNB") && !Starts("UNH")) throw Fail(FerruleEdiError.Format, "Invalid EDI opening segment.");
                syntax = canonical with { ReadComponent = component.ToString() };
                boundary = false;
            }
            int start = at;
            while (at < text.Length)
            {
                Rune rune = Rune.GetRuneAt(text, at);
                if (rune == terminator) break;
                at += rune.Utf16SequenceLength;
                if (rune == release)
                {
                    if (at == text.Length) throw Fail(FerruleEdiError.Format, "Dangling EDI release character.");
                    at += Rune.GetRuneAt(text, at).Utf16SequenceLength;
                }
            }
            string raw = text[start..at];
            if (at < text.Length) at += terminator.Utf16SequenceLength;
            foreach (var parsed in Delimited(raw, syntax, false, true, advice, element, component, terminator, release, repetition))
            {
                segments.Add(parsed with { ReadSyntax = syntax });
                // Framing owns syntax only; strict envelope validation still owns
                // control values, counts, references and invalid nesting.
                switch (parsed.Id)
                {
                    case "UNB": interchange = true; repetition = IsEdifactV4(parsed.Elements.FirstOrDefault()?.FirstOrDefault()?.ElementAtOrDefault(1)) ? advice : null; break;
                    case "UNG": group = true; break;
                    case "UNH": message = true; break;
                    case "UNT": message = false; boundary = !interchange && !group; break;
                    case "UNE": group = false; break;
                    case "UNZ": interchange = false; boundary = !message && !group; break;
                }
            }
        }
        if (segments.Count == 0) throw Fail(FerruleEdiError.Format, "Invalid EDI opening segment.");
        return (segments, canonical);
    }

    private (List<Segment>, Syntax) X12Segments(string text)
    {
        int discoveryWork = checked(text.Length * 2);
        Syntax first = ReadX12Syntax(text, 0, ref discoveryWork), syntax = first;
        var segments = new List<Segment>();
        int at = 0;
        while (at < text.Length)
        {
            while (at < text.Length && char.IsWhiteSpace(text[at])) at++;
            if (at == text.Length) break;
            if (segments.Count != 0 && text.AsSpan(at).StartsWith("ISA", StringComparison.Ordinal)
                && (at + 3 == text.Length || !char.IsLetterOrDigit(text[at + 3])))
                syntax = ReadX12Syntax(text, at, ref discoveryWork);
            int end = at;
            while (end < text.Length)
            {
                if (text[end] == syntax.Release)
                {
                    if (++end == text.Length) throw Fail(FerruleEdiError.Format, "Dangling EDI release character.");
                }
                else if (text[end] == syntax.Terminator) break;
                end++;
            }
            string raw = text[at..end];
            at = end < text.Length ? end + 1 : end;
            foreach (var segment in Delimited(raw, syntax, false))
                segments.Add(segment with { ReadSyntax = syntax });
        }
        return (segments, first);
    }

    private Syntax ReadX12Syntax(string text, int start, ref int discoveryWork)
    {
        if (text.Length - start < 4 || !text.AsSpan(start).StartsWith("ISA", StringComparison.Ordinal)) throw Fail(FerruleEdiError.Format, "X12 must start with ISA.");
        char element = text[start + 3];
        var found = new List<int>(16);
        for (int at = start; at < text.Length && found.Count < 16; at++)
        {
            if (--discoveryWork < 0) throw Fail(FerruleEdiError.Format, "ISA syntax discovery work limit exceeded.");
            if (text[at] == element) found.Add(at);
        }
        int[] positions = found.ToArray();
        if (positions.Length != 16 || positions[15] + 2 >= text.Length) throw Fail(FerruleEdiError.Format, "Truncated ISA segment.");
        string version = text[(positions[11] + 1)..positions[12]], isa11 = text[(positions[10] + 1)..positions[11]];
        if (version.Length != 5 || !version.All(char.IsAsciiDigit)) throw Fail(FerruleEdiError.Format, "Invalid ISA12 version.");
        char? repeat = string.CompareOrdinal(version, "00501") >= 0 ? isa11.Length == 1 && !char.IsLetterOrDigit(isa11[0]) ? isa11[0] : throw Fail(FerruleEdiError.Format, "Invalid ISA11 repetition separator.") : null;
        if (repeat is null && (isa11.Length == 0 || !isa11.All(char.IsLetterOrDigit))) throw Fail(FerruleEdiError.Format, "Legacy ISA11 must be alphanumeric.");
        var syntax = new Syntax(element, text[positions[15] + 1], text[positions[15] + 2], null, repeat);
        if (_options.TryGetProperty("x12_separators", out var configured) && configured.ValueKind == System.Text.Json.JsonValueKind.Object)
        {
            if (Text(configured, "element") != syntax.Element.ToString() || Text(configured, "component") != syntax.Component.ToString() || Text(configured, "segment") != syntax.Terminator.ToString()
                || Text(configured, "repetition") is { } expected && repeat is { } actual && expected != actual.ToString())
                throw Fail(FerruleEdiError.Format, "X12 separators do not match the configured syntax.");
            syntax = syntax with { Release = Text(configured, "release")?.Single() };
        }
        return syntax;
    }

    private static (List<Segment>, Syntax) Hl7Segments(string text)
    {
        string[] lines = text.Split(['\r', '\n']).Where(line => !string.IsNullOrWhiteSpace(line)).ToArray();
        if (lines.Length == 0 || lines[0].Length < 8 || lines[0][..3] is not ("FHS" or "BHS" or "MSH")) throw Fail(FerruleEdiError.Format, "HL7 stream must start with a header.");
        char field = lines[0][3];
        string encoding = lines[0][4..].Split(field)[0];
        if (encoding.Length < 4) throw Fail(FerruleEdiError.Format, "Missing HL7 encoding characters.");
        var syntax = new Syntax(field, encoding[0], '\r', encoding[2], encoding[1], encoding[3]);
        if (new[] { field, encoding[0], encoding[1], encoding[2], encoding[3] }.Distinct().Count() != 5) throw Fail(FerruleEdiError.Format, "HL7 delimiters must be distinct.");
        var result = new List<Segment>();
        foreach (string line in lines)
        {
            if (line.Length < 4 || !line[..3].All(char.IsAsciiLetterOrDigit) || line[3] != field) throw Fail(FerruleEdiError.Format, "Invalid HL7 segment.");
            string id = line[..3];
            var parts = line[4..].Split(field).ToList();
            var elements = new List<List<List<string>>>();
            if (id is "FHS" or "BHS" or "MSH")
            {
                elements.Add([[field.ToString()]]); elements.Add([[parts[0]]]); parts.RemoveAt(0);
            }
            elements.AddRange(parts.Select(part => part.Split(encoding[1]).Select(repeat => repeat.Split(encoding[0]).Select(component => Hl7Unescape(component, syntax)).ToList()).ToList()));
            result.Add(new(id, elements));
        }
        return (result, syntax);
    }
    private static string Hl7Unescape(string text, Syntax syntax)
    {
        char escape = syntax.Release!.Value;
        string[] parts = text.Split(escape);
        var output = new StringBuilder(parts[0]);
        for (int at = 1; at < parts.Length; at++)
        {
            if (at % 2 == 0) { output.Append(parts[at]); continue; }
            output.Append(parts[at] switch { "F" => syntax.Element.ToString(), "S" => syntax.Component.ToString(), "R" => syntax.Repetition.ToString(), "T" => $"{escape}T{escape}", "E" => escape.ToString(), _ => escape + parts[at] });
        }
        return output.ToString();
    }

    private List<Segment> IdocSegments(byte[] bytes)
    {
        var segments = new List<Segment>();
        int records = 0, start = 0;
        for (int end = 0; end <= bytes.Length; end++)
        {
            if (end < bytes.Length && bytes[end] is not (10 or 13)) continue;
            var record = bytes.AsSpan(start, end - start); start = end + 1;
            if (record.StartsWith(new byte[] { 0xef, 0xbb, 0xbf })) record = record[3..];
            while (record.Length > 0 && record[^1] is 0 or 9 or 32) record = record[..^1];
            if (record.IsEmpty) continue;
            if (++records > 100_000) throw Fail(FerruleEdiError.Limit, "IDoc record limit exceeded.");
            System.Text.Json.JsonElement? layout = null;
            foreach (var entry in _layouts)
            {
                byte[] name = Utf8.GetBytes(entry.Key);
                if (record.StartsWith(name) && (record.Length == name.Length || record[name.Length] is 9 or 10 or 11 or 12 or 13 or 32)) { layout = entry.Value; break; }
            }
            if (layout is null)
            {
                if (record.StartsWith("EDI_DC40"u8) || _lenient) continue;
                throw Fail(FerruleEdiError.Format, "Unrecognized IDoc segment.");
            }
            var elements = new List<List<List<string>>>();
            foreach (var field in Array(layout.Value, "fields"))
            {
                int first = field.GetProperty("first_byte").GetInt32() - 1, last = field.GetProperty("last_byte").GetInt32();
                var value = first < record.Length ? record[first..Math.Min(record.Length, last)] : [];
                while (value.Length > 0 && value[0] is 0 or 9 or 32) value = value[1..];
                while (value.Length > 0 && value[^1] is 0 or 9 or 32) value = value[..^1];
                elements.Add([[Utf8.GetString(value)]]);
            }
            segments.Add(new(Text(layout.Value, "name")!, elements));
        }
        return segments;
    }
}
