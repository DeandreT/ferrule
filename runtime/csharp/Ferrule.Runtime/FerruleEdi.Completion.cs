using System.Globalization;
using System.Text.Json;

namespace Ferrule.Runtime;

public sealed partial class FerruleEdi
{
    private sealed class Envelope(string reference)
    {
        internal string Reference { get; } = reference;
        internal int Messages { get; set; }
        internal int Groups { get; set; }
    }
    private List<OutputSegment> Complete(List<OutputSegment> segments, JsonElement configuration, string? datetime, Syntax syntax)
    {
        if (_kind is not ("X12" or "Edifact")) return segments;
        if (segments.Count > 1_000_000) throw Fail(FerruleEdiError.Limit, "EDI completion segment limit exceeded.");
        if (datetime is null || datetime.Length < 19 || datetime[4] != '-' || datetime[7] != '-' || datetime[10] != 'T' || datetime[13] != ':' || datetime[16] != ':' || datetime[..19].Where((_, at) => at is not (4 or 7 or 10 or 13 or 16)).Any(c => !char.IsAsciiDigit(c)))
            throw Fail(FerruleEdiError.Value, "Autocomplete requires a stable YYYY-MM-DDTHH:MM:SS timestamp.");
        var config = configuration.TryGetProperty(_kind == "X12" ? "x12" : "edifact", out var wrapped) ? wrapped : configuration;
        bool x12 = _kind == "X12";
        string date8 = datetime[..4] + datetime[5..7] + datetime[8..10], date6 = date8[2..], time4 = datetime[11..13] + datetime[14..16], time6 = time4 + datetime[17..19];
        string headerId = x12 ? "ISA" : "UNB", groupId = x12 ? "GS" : "UNG", messageId = x12 ? "ST" : "UNH";
        string messageEnd = x12 ? "SE" : "UNT", groupEnd = x12 ? "GE" : "UNE", interchangeEnd = x12 ? "IEA" : "UNZ";
        var output = new List<OutputSegment>();
        Envelope? interchange = null, group = null;
        (int Start, string Reference)? message = null;
        int groupOrdinal = 0, messageOrdinal = 0, interchangeOrdinal = 0;
        void Push(OutputSegment segment)
        {
            if (output.Count >= 1_000_000) throw Fail(FerruleEdiError.Limit, "EDI completion segment limit exceeded.");
            output.Add(segment);
        }
        void FinishMessage(OutputSegment? trailer = null)
        {
            if (message is not { } active) { if (trailer is not null) throw Fail(FerruleEdiError.Schema, "Orphan message trailer."); return; }
            trailer ??= new(messageEnd, []);
            Ensure(trailer, 0, (output.Count - active.Start + 1).ToString(CultureInfo.InvariantCulture)); Ensure(trailer, 1, active.Reference);
            Push(trailer); message = null;
            if (x12 && group is null) throw Fail(FerruleEdiError.Schema, "Transaction is not owned by a functional group.");
            if (group is not null) group.Messages++;
            if (interchange is not null) interchange.Messages++;
        }
        void FinishGroup(OutputSegment? trailer = null)
        {
            if (group is null) { if (trailer is not null) throw Fail(FerruleEdiError.Schema, "Orphan group trailer."); return; }
            if (interchange is null) throw Fail(FerruleEdiError.Schema, "Group is not owned by an interchange.");
            trailer ??= new(groupEnd, []);
            Ensure(trailer, 0, group.Messages.ToString(CultureInfo.InvariantCulture)); Ensure(trailer, 1, group.Reference);
            Push(trailer); group = null; interchange.Groups++;
        }
        void FinishInterchange(OutputSegment? trailer = null)
        {
            if (interchange is null) { if (trailer is not null) throw Fail(FerruleEdiError.Schema, "Orphan interchange trailer."); return; }
            trailer ??= new(interchangeEnd, []);
            Ensure(trailer, 0, (x12 || interchange.Groups != 0 ? interchange.Groups : interchange.Messages).ToString(CultureInfo.InvariantCulture));
            Ensure(trailer, 1, interchange.Reference); Push(trailer); interchange = null;
        }
        foreach (var segment in segments)
        {
            if (segment.Id == headerId)
            {
                FinishMessage(); FinishGroup(); FinishInterchange(); interchangeOrdinal++;
                if (x12)
                {
                    string[] defaults = ["00", "          ", "00", "          ", "ZZ", "", "ZZ", "", date6, time4, "", "", "000000000", Flag(config, "request_acknowledgement") ? "1" : "0", "P", ""];
                    for (int at = 0; at < defaults.Length; at++) Ensure(segment, at, defaults[at]);
                    Compact(segment, 8, "compact_date6"); Compact(segment, 9, "compact_time", 4, 4);
                    if (segment.Elements[12].Length is 0 or > 9 || !segment.Elements[12].All(char.IsAsciiDigit)) throw Fail(FerruleEdiError.Value, "ISA control number requires at most nine digits.");
                    segment.Elements[12] = segment.Elements[12].PadLeft(9, '0');
                }
                else
                {
                    string? agency = Text(config, "controlling_agency"), level = Text(config, "syntax_level");
                    if (agency is not null && level is not null) EnsureComponents(segment, 0, [agency + level, Text(config, "syntax_version") ?? ""], syntax);
                    else if (Text(config, "syntax_version") is { } selectedVersion) EnsureComponents(segment, 0, ["", selectedVersion], syntax);
                    string version = segment.Elements.Count > 0 ? segment.Elements[0].Split(syntax.Component).ElementAtOrDefault(1) ?? "" : "";
                    syntax = syntax with { Repetition = IsEdifactV4(version, DefaultEdifactV4()) ? '*' : null };
                    EnsureComponents(segment, 3, [string.CompareOrdinal(version, "4") >= 0 ? date8 : date6, time4], syntax);
                }
                interchange = new(Ensure(segment, x12 ? 12 : 4, x12 ? "000000000" : interchangeOrdinal.ToString(CultureInfo.InvariantCulture)));
                Push(segment);
            }
            else if (segment.Id == groupId)
            {
                FinishMessage(); FinishGroup();
                if (interchange is null) throw Fail(FerruleEdiError.Schema, "Functional group precedes interchange.");
                if (x12)
                {
                    Ensure(segment, 3, date8); Ensure(segment, 4, time6);
                    Compact(segment, 3, "compact_date8"); Compact(segment, 4, "compact_time", 4, 8);
                }
                group = new(Ensure(segment, x12 ? 5 : 4, (++groupOrdinal).ToString(CultureInfo.InvariantCulture))); Push(segment);
            }
            else if (segment.Id == messageId)
            {
                FinishMessage();
                if (x12 && group is null) throw Fail(FerruleEdiError.Schema, "Transaction precedes functional group.");
                if (x12) Ensure(segment, 0, Escape(Text(config, "transaction_set") ?? "", syntax, null));
                else
                {
                    string agency = Text(config, "controlling_agency") ?? "";
                    if (agency.EndsWith('O')) agency = agency[..^1];
                    EnsureComponents(segment, 1, [Text(config, "message_type") ?? "", "", "", agency], syntax);
                }
                string reference = Ensure(segment, x12 ? 1 : 0, (++messageOrdinal).ToString(x12 ? "D4" : "D", CultureInfo.InvariantCulture));
                message = (output.Count, reference); Push(segment);
            }
            else if (segment.Id == messageEnd) FinishMessage(segment);
            else if (segment.Id == groupEnd) { FinishMessage(); FinishGroup(segment); }
            else if (segment.Id == interchangeEnd) { FinishMessage(); FinishGroup(); FinishInterchange(segment); }
            else Push(segment);
        }
        FinishMessage(); FinishGroup(); FinishInterchange();
        return output;
    }
    private static string Ensure(OutputSegment segment, int index, string fallback)
    {
        while (segment.Elements.Count <= index) segment.Elements.Add("");
        if (segment.Elements[index].Length == 0) segment.Elements[index] = fallback;
        return segment.Elements[index];
    }
    private void EnsureComponents(OutputSegment segment, int index, string[] defaults, Syntax syntax)
    {
        string value = Ensure(segment, index, "");
        var components = Split(value, syntax.Component, syntax.Release);
        while (components.Count < defaults.Length) components.Add("");
        for (int at = 0; at < defaults.Length; at++) if (components[at].Length == 0) components[at] = Escape(defaults[at], syntax, null);
        segment.Elements[index] = string.Join(syntax.Component, components);
    }
    private static void Compact(OutputSegment segment, int index, string kind, byte minimum = 0, byte maximum = 0)
    {
        string value = Ensure(segment, index, "");
        segment.Elements[index] = FormatLexical(FerruleValue.FromString(value), kind, minimum, maximum, 0).StringValue;
    }
}
