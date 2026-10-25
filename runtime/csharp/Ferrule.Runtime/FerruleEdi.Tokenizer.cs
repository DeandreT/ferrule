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
        var syntax = new Syntax('+', ':', '\'', '?', null);
        char repeatAdvice = '*';
        text = _kind == "Tradacoms" ? text.TrimStart('\ufeff') : text.TrimStart();
        if (_kind == "Edifact" && text.StartsWith("UNA", StringComparison.Ordinal))
        {
            if (text.Length < 9) throw Fail(FerruleEdiError.Format, "Truncated UNA service string.");
            syntax = new(text[4], text[3], text[8], text[6], null);
            repeatAdvice = text[7];
            text = text[9..].TrimStart();
        }
        List<Segment> segments = Delimited(text, syntax, _kind == "Tradacoms", _kind == "Edifact", repeatAdvice);
        if (segments.Count == 0 || (_kind == "Tradacoms" ? segments[0].Id != "STX" : segments[0].Id is not ("UNB" or "UNH")))
            throw Fail(FerruleEdiError.Format, "Invalid EDI opening segment.");
        return (segments, syntax);
    }

    // Retain release pairs until the innermost component is decoded. This
    // prevents escaped separators from being split a second time.
    private static List<string> Split(string text, char delimiter, char? release)
    {
        var result = new List<string>();
        int start = 0;
        for (int at = 0; at < text.Length; at++)
        {
            if (text[at] == release)
            {
                if (++at == text.Length) throw Fail(FerruleEdiError.Format, "Dangling EDI release character.");
            }
            else if (text[at] == delimiter) { result.Add(text[start..at]); start = at + 1; }
        }
        result.Add(text[start..]);
        return result;
    }
    private static string Unescape(string text, char? release)
    {
        if (release is null) return text;
        var output = new StringBuilder();
        for (int at = 0; at < text.Length; at++)
        {
            if (text[at] == release && ++at == text.Length) throw Fail(FerruleEdiError.Format, "Dangling EDI release character.");
            output.Append(text[at]);
        }
        return output.ToString();
    }
    private static List<Segment> Delimited(string text, Syntax syntax, bool assigned, bool edifact = false, char repeatAdvice = '*')
    {
        var segments = new List<Segment>();
        foreach (string raw in Split(text, syntax.Terminator, syntax.Release))
        {
            string segment = raw.TrimStart();
            if (segment.Length == 0) continue;
            string id;
            List<string> elements;
            if (assigned)
            {
                var pair = Split(segment, '=', syntax.Release);
                if (pair.Count < 2) throw Fail(FerruleEdiError.Format, "TRADACOMS segment has no assignment separator.");
                id = Unescape(pair[0].Trim(), syntax.Release);
                if (id.Length != 3 || !id.All(char.IsAsciiLetterOrDigit)) throw Fail(FerruleEdiError.Format, "Invalid TRADACOMS segment ID.");
                elements = Split(string.Join('=', pair.Skip(1)), syntax.Element, syntax.Release);
            }
            else
            {
                elements = Split(segment, syntax.Element, syntax.Release);
                id = elements[0]; elements.RemoveAt(0);
            }
            var parsed = elements.Select(element => id == "ISA" ? new List<List<string>> { new() { element } } :
                (syntax.Repetition is { } repeat && !(edifact && id == "UNB") ? Split(element, repeat, syntax.Release) : [element])
                .Select(value => Split(value, syntax.Component, syntax.Release).Select(part => Unescape(part, syntax.Release)).ToList()).ToList()).ToList();
            segments.Add(new(id, parsed));
            if (edifact && id == "UNB")
            {
                string? version = parsed.FirstOrDefault()?.FirstOrDefault()?.ElementAtOrDefault(1);
                bool v4 = IsEdifactV4(version);
                if (v4 && (char.IsWhiteSpace(repeatAdvice) || new char?[] { syntax.Element, syntax.Component, syntax.Terminator, syntax.Release }.Contains(repeatAdvice)))
                    throw Fail(FerruleEdiError.Format, "Syntax v4 requires a distinct visible repetition separator.");
                syntax = syntax with { Repetition = v4 ? repeatAdvice : null };
            }
        }
        return segments;
    }

    private static bool IsEdifactV4(string? version, bool fallback = false) =>
        uint.TryParse(version, System.Globalization.NumberStyles.AllowLeadingSign, System.Globalization.CultureInfo.InvariantCulture, out uint number) ? number >= 4 : fallback;

    private (List<Segment>, Syntax) X12Segments(string text)
    {
        if (!text.StartsWith("ISA", StringComparison.Ordinal) || text.Length < 4) throw Fail(FerruleEdiError.Format, "X12 must start with ISA.");
        char element = text[3];
        int[] positions = text.Select((c, at) => (c, at)).Where(pair => pair.c == element).Take(16).Select(pair => pair.at).ToArray();
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
        return (Delimited(text, syntax, false), syntax);
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
